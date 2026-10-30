//! Tests that a real Stray process gives the shell the right size as the window changes.

#![cfg(unix)]

use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use std::borrow::Cow;
use std::io::{ErrorKind, Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use stray_cat::bar::MIN_TERMINAL_ROWS;
use stray_cat::escape::{
    DEVICE_ATTRIBUTES_QUERY, RESET_SCROLL_REGION, RESTORE_TERMINAL, scroll_region,
};

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

/// Says images are supported, the cursor is in the top left, and the check is over.
const SUPPORTED_TERMINAL_REPLIES: &[u8] = b"\x1b_Gi=3927641;OK\x1b\\\x1b[1;1R\x1b[?62;22c";

const TIMEOUT: Duration = Duration::from_secs(5);

/// Exit code of a process stopped by SIGTERM (128 + 15).
const TERMINATED_EXIT_CODE: u32 = 143;

fn size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Stops Stray if a test ends early.
struct TestSession {
    child: Box<dyn Child + Send + Sync>,
}

impl Drop for TestSession {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Reads output until `expected` appears.
fn wait_for_output(output: &Receiver<Vec<u8>>, expected: &[u8]) -> Result {
    let deadline = Instant::now() + TIMEOUT;
    let mut received = Vec::new();

    loop {
        let chunk = output
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| {
                format!(
                    "missing {:?}: {error}; output: {}",
                    String::from_utf8_lossy(expected),
                    String::from_utf8_lossy(&received)
                )
            })?;
        received.extend_from_slice(&chunk);
        if contains(&received, expected) {
            return Ok(());
        }
    }
}

fn contains(bytes: &[u8], part: &[u8]) -> bool {
    bytes.windows(part.len()).any(|window| window == part)
}

#[test]
fn start_forwards_resizes_and_preserves_shell_exit_code() -> Result {
    check_session(false)
}

#[test]
fn termination_restores_the_scroll_region() -> Result {
    check_session(true)
}

/// Runs Stray in a fake terminal, resizes it a few times, and checks the shell's size.
/// Then ends the shell, or stops Stray with SIGTERM when `terminate` is true.
fn check_session(terminate: bool) -> Result {
    let pty = native_pty_system().openpty(size(24, 80))?;
    let mut reader = pty.master.try_clone_reader()?;
    let mut writer = pty.master.take_writer()?;
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_stray"));
    command.arg("start");
    command.env("SHELL", "/bin/sh");
    command.env("ENV", "/dev/null");
    command.env_remove("TMUX");
    let mut session = TestSession {
        child: pty.slave.spawn_command(command)?,
    };
    drop(pty.slave);

    let (sender, output) = mpsc::channel();
    let reader_thread = thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut received = Vec::new();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    received.extend_from_slice(&buffer[..count]);
                    if sender.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        received
    });

    wait_for_output(&output, DEVICE_ATTRIBUTES_QUERY)?;
    writer.write_all(SUPPORTED_TERMINAL_REPLIES)?;

    for (rows, cols, shell_rows) in [
        (24, 80, 22),
        (40, 120, 38),
        (9, 30, 9),
        (10, 30, 8),
        (24, 80, 22),
    ] {
        pty.master.resize(size(rows, cols))?;

        // Short terminals have no bar, so the whole screen scrolls.
        let region = if rows < MIN_TERMINAL_ROWS {
            Cow::Borrowed(RESET_SCROLL_REGION)
        } else {
            Cow::Owned(scroll_region(shell_rows).into_bytes())
        };
        wait_for_output(&output, &region)?;

        // Allow the session loop time to resize, with a bounded shell loop.
        writeln!(
            writer,
            "for attempt in 1 2 3 4 5 6 7 8 9 10; do size=$(stty size); if [ \"$size\" = '{shell_rows} {cols}' ]; then printf '\\nSTRAY_SIZE_%s_DONE\\n' \"$size\"; break; fi; sleep 0.05; done"
        )?;
        writer.flush()?;
        let done = format!("STRAY_SIZE_{shell_rows} {cols}_DONE");
        wait_for_output(&output, done.as_bytes())?;
    }

    if terminate {
        let pid = session
            .child
            .process_id()
            .ok_or("Stray should have a process ID")?;
        let status = std::process::Command::new("/bin/kill")
            .args(["-TERM", &pid.to_string()])
            .status()?;
        assert!(status.success());
    } else {
        writeln!(writer, "exit 7")?;
        writer.flush()?;
    }
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = session.child.try_wait()? {
            break status;
        }
        assert!(Instant::now() < deadline, "Stray should exit promptly");
        thread::sleep(Duration::from_millis(10));
    };

    let expected_code = if terminate { TERMINATED_EXIT_CODE } else { 7 };
    assert_eq!(status.exit_code(), expected_code);
    let received = reader_thread
        .join()
        .map_err(|_| "output reader thread panicked")?;
    assert!(contains(&received, &RESTORE_TERMINAL.concat()));
    Ok(())
}
