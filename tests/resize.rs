#![cfg(unix)]

use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use std::io::{ErrorKind, Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

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

fn wait_for_output(output: &Receiver<Vec<u8>>, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut received = Vec::new();

    loop {
        let chunk = output
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|error| {
                panic!(
                    "missing {expected:?}: {error}; output: {}",
                    String::from_utf8_lossy(&received)
                )
            });
        received.extend_from_slice(&chunk);
        if received
            .windows(expected.len())
            .any(|window| window == expected.as_bytes())
        {
            return;
        }
    }
}

#[test]
fn start_forwards_resizes_and_preserves_shell_exit_code() {
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("outer PTY should open");
    let mut reader = pty.master.try_clone_reader().expect("reader should open");
    let mut writer = pty.master.take_writer().expect("writer should open");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_stray"));
    command.arg("start");
    command.env("SHELL", "/bin/sh");
    command.env("ENV", "/dev/null");
    let mut session = TestSession {
        child: pty
            .slave
            .spawn_command(command)
            .expect("Stray should start"),
    };
    drop(pty.slave);

    let (sender, output) = mpsc::channel();
    let reader_thread = thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if sender.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });

    for (rows, cols) in [(24, 80), (40, 120), (10, 30), (24, 80)] {
        pty.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("outer PTY should resize");

        // Allow the resize thread time to run, with a bounded shell loop.
        writeln!(
            writer,
            "for attempt in 1 2 3 4 5 6 7 8 9 10; do size=$(stty size); if [ \"$size\" = '{rows} {cols}' ]; then printf '\\nSTRAY_SIZE_%s_DONE\\n' \"$size\"; break; fi; sleep 0.05; done"
        )
        .expect("size check should reach shell");
        writer.flush().expect("size check should flush");
        wait_for_output(&output, &format!("STRAY_SIZE_{rows} {cols}_DONE"));
    }

    writeln!(writer, "exit 7").expect("exit command should reach shell");
    writer.flush().expect("exit command should flush");
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = session.child.try_wait().expect("Stray should be waitable") {
            break status;
        }
        assert!(Instant::now() < deadline, "Stray should exit promptly");
        thread::sleep(Duration::from_millis(10));
    };

    assert_eq!(status.exit_code(), 7);
    reader_thread.join().expect("output reader should finish");
}
