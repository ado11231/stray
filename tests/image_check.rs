#![cfg(unix)]

use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use stray_cat::escape::DEVICE_ATTRIBUTES_QUERY;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// What a terminal that can show images answers to Stray's image check.
const IMAGES_SUPPORTED_REPLY: &[u8] = b"\x1b_Gi=3927641;OK\x1b\\";

/// What every terminal answers last, which ends the image check.
const ATTRIBUTES_REPLY: &[u8] = b"\x1b[?1;2c";

/// The prompt of the test shell.
const PROMPT: &str = "PROBE_READY> ";

/// The hint Stray shows when images are not available.
const HINT: &str = "images unavailable";

/// Typed before the check ends. The shell should still receive it.
const EARLY_COMMAND: &[u8] = b"printf '\\nEARLY:%s\\n' kept\n";
const EARLY_OUTPUT: &[u8] = b"\r\nEARLY:kept\r\n";

const TIMEOUT: Duration = Duration::from_secs(5);

// Tests run in parallel, so the clock alone can give two tests the same folder.
static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

/// A fake terminal running `stray start`, which the test answers by hand.
struct Terminal {
    child: Box<dyn Child + Send + Sync>,
    input: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    received: Vec<u8>,
    directory: PathBuf,
}

impl Terminal {
    /// Starts Stray and waits until it asks its questions.
    fn start() -> Result<Self> {
        let directory = std::env::temp_dir().join(format!(
            "stray-probe-{}-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        std::fs::create_dir(&directory)?;
        let pty = native_pty_system().openpty(PtySize::default())?;
        let mut reader = pty.master.try_clone_reader()?;
        let input = pty.master.take_writer()?;
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_stray"));
        command.args(["start", "--shell", "/bin/sh"]);
        command.env("HOME", &directory);
        command.env("XDG_DATA_HOME", &directory);
        command.env("ENV", "/dev/null");
        command.env("PS1", PROMPT);
        command.env_remove("TMUX");
        let child = pty.slave.spawn_command(command)?;
        drop(pty.slave);
        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            // Keep the master open until the child closes its terminal.
            let _master = pty.master;
            let mut bytes = [0; 4096];
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if sender.send(bytes[..count].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let mut terminal = Self {
            child,
            input,
            output,
            received: Vec::new(),
            directory,
        };
        terminal.wait_for(DEVICE_ATTRIBUTES_QUERY)?;
        Ok(terminal)
    }

    /// Types bytes into the terminal, or answers Stray's questions.
    fn send(&mut self, bytes: &[u8]) -> Result {
        self.input.write_all(bytes)?;
        self.input.flush()?;
        Ok(())
    }

    /// Reads output until `expected` appears.
    fn wait_for(&mut self, expected: &[u8]) -> Result {
        let deadline = Instant::now() + TIMEOUT;
        while !self
            .received
            .windows(expected.len())
            .any(|bytes| bytes == expected)
        {
            let bytes = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|error| format!("{error}: {}", String::from_utf8_lossy(&self.received)))?;
            self.received.extend(bytes);
        }
        Ok(())
    }

    fn received_text(&self) -> String {
        String::from_utf8_lossy(&self.received).into_owned()
    }

    /// Checks whether the shell runs inside Stray or directly, then exits it.
    fn check_shell(&mut self, wrapped: bool) -> Result {
        self.wait_for(PROMPT.as_bytes())?;
        let pid = self
            .child
            .process_id()
            .ok_or("Stray should have a process ID")?;
        // The shell replaced Stray only when it has Stray's process ID.
        self.send(format!(
            "if [ \"$$\" = {pid} ]; then printf '\\nMODE:direct\\n'; else printf '\\nMODE:wrapped\\n'; fi\n"
        ).as_bytes())?;
        self.wait_for(if wrapped {
            b"\r\nMODE:wrapped\r\n"
        } else {
            b"\r\nMODE:direct\r\n"
        })?;
        self.send(b"exit 7\n")?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                assert_eq!(status.exit_code(), 7);
                return Ok(());
            }
            assert!(Instant::now() < deadline, "shell did not exit");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn supported_terminal_wraps_the_shell_and_preserves_early_input() -> Result {
    let mut terminal = Terminal::start()?;
    terminal.send(&[EARLY_COMMAND, IMAGES_SUPPORTED_REPLY, b"\x1b[?62;22c"].concat())?;
    terminal.wait_for(EARLY_OUTPUT)?;
    terminal.check_shell(true)?;
    assert!(!terminal.received_text().contains(HINT));
    Ok(())
}

#[test]
fn unsupported_terminal_runs_the_shell_directly() -> Result {
    let mut terminal = Terminal::start()?;
    terminal.send(ATTRIBUTES_REPLY)?;
    terminal.check_shell(false)?;
    assert!(terminal.received_text().contains(HINT));
    Ok(())
}

#[test]
fn unsupported_terminal_keeps_a_plain_wrapper_when_keys_were_read() -> Result {
    let mut terminal = Terminal::start()?;
    terminal.send(&[EARLY_COMMAND, ATTRIBUTES_REPLY].concat())?;
    terminal.wait_for(EARLY_OUTPUT)?;
    terminal.check_shell(true)
}

#[test]
fn silent_terminal_times_out_and_runs_the_shell_directly() -> Result {
    let mut terminal = Terminal::start()?;
    terminal.check_shell(false)
}
