#![cfg(unix)]

use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Terminal {
    child: Box<dyn Child + Send + Sync>,
    input: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    received: Vec<u8>,
    directory: PathBuf,
}

impl Terminal {
    fn start() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "stray-probe-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let pty = native_pty_system().openpty(PtySize::default()).unwrap();
        let mut reader = pty.master.try_clone_reader().unwrap();
        let input = pty.master.take_writer().unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_stray"));
        command.args(["start", "--shell", "/bin/sh"]);
        command.env("HOME", &directory);
        command.env("XDG_DATA_HOME", &directory);
        command.env("ENV", "/dev/null");
        command.env("PS1", "PROBE_READY> ");
        command.env_remove("TMUX");
        let child = pty.slave.spawn_command(command).unwrap();
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
        terminal.wait_for(b"\x1b[c");
        terminal
    }

    fn send(&mut self, bytes: &[u8]) {
        self.input.write_all(bytes).unwrap();
        self.input.flush().unwrap();
    }

    fn wait_for(&mut self, expected: &[u8]) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self
            .received
            .windows(expected.len())
            .any(|bytes| bytes == expected)
        {
            let bytes = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| {
                    panic!("{error}: {}", String::from_utf8_lossy(&self.received))
                });
            self.received.extend(bytes);
        }
    }

    fn check_shell(&mut self, wrapped: bool) {
        self.wait_for(b"PROBE_READY> ");
        let pid = self.child.process_id().unwrap();
        self.send(format!(
            "if [ \"$$\" = {pid} ]; then printf '\\nMODE:direct\\n'; else printf '\\nMODE:wrapped\\n'; fi\n"
        ).as_bytes());
        self.wait_for(if wrapped {
            b"\r\nMODE:wrapped\r\n"
        } else {
            b"\r\nMODE:direct\r\n"
        });
        self.send(b"exit 7\n");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.exit_code(), 7);
                break;
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
fn supported_terminal_wraps_the_shell_and_preserves_early_input() {
    let mut terminal = Terminal::start();
    terminal.send(b"printf '\\nEARLY:%s\\n' kept\n\x1b_Gi=3927641;OK\x1b\\\x1b[?62;22c");
    terminal.wait_for(b"\r\nEARLY:kept\r\n");
    terminal.check_shell(true);
    assert!(!String::from_utf8_lossy(&terminal.received).contains("images unavailable"));
}

#[test]
fn unsupported_terminal_runs_the_shell_directly() {
    let mut terminal = Terminal::start();
    terminal.send(b"\x1b[?1;2c");
    terminal.check_shell(false);
    assert!(String::from_utf8_lossy(&terminal.received).contains("images unavailable"));
}

#[test]
fn unsupported_terminal_keeps_a_plain_wrapper_when_keys_were_read() {
    let mut terminal = Terminal::start();
    terminal.send(b"printf '\\nEARLY:%s\\n' kept\n\x1b[?1;2c");
    terminal.wait_for(b"\r\nEARLY:kept\r\n");
    terminal.check_shell(true);
}

#[test]
fn silent_terminal_times_out_and_runs_the_shell_directly() {
    let mut terminal = Terminal::start();
    terminal.check_shell(false);
}
