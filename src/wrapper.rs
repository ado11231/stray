use crate::terminal;
use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtyPair, PtySize, native_pty_system};
use signal_hook::iterator::{Handle, Signals};
use std::ffi::OsStr;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};
use std::thread::{self, JoinHandle};

/// Stops and collects the shell when session leaves scope.
pub struct ShellGuard {
    pub child: Box<dyn Child + Send + Sync>,
}

impl Drop for ShellGuard {
    fn drop(&mut self) {
        // A shell that already finished does not need further cleanup.
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        // Stops the shell, then collects exit status.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct ResizeGuard {
    signal_handle: Handle,
    thread: Option<JoinHandle<Result<()>>>,
}

impl ResizeGuard {
    pub fn stop(&mut self) -> Result<()> {
        self.signal_handle.close();

        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("resize thread panicked"))??;
        }

        Ok(())
    }
}

impl Drop for ResizeGuard {
    fn drop(&mut self) {
        // Cleanup must not panic while another error is being handled.
        let _ = self.stop();
    }
}

pub struct ShellSession {
    shell: ShellGuard,
    master: Box<dyn MasterPty + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    signals: Signals,
    _raw_mode: terminal::RawModeGuard,
}

impl ShellSession {
    pub fn start(command: CommandBuilder) -> Result<Self> {
        let signals = terminal::watch_resize()?;
        let size = terminal::read_size()?;
        let pty = open_pty(size)?;

        let reader = pty
            .master
            .try_clone_reader()
            .context("could not open PTY reader")?;

        let writer = pty
            .master
            .take_writer()
            .context("could not open PTY writer")?;

        let raw_mode = terminal::RawModeGuard::enable()?;

        let shell = ShellGuard {
            child: spawn_shell(&pty, command)?,
        };
        drop(pty.slave);

        Ok(Self {
            shell,
            master: pty.master,
            reader,
            writer,
            signals,
            _raw_mode: raw_mode,
        })
    }

    pub fn run(mut self) -> Result<ExitCode> {
        let mut resize_guard = start_resize_thread(self.master, self.signals)?;
        let _input_thread = start_input_thread(self.writer)?;

        let stdout = std::io::stdout();

        forward_output(self.reader, stdout.lock())?;

        let status = self
            .shell
            .child
            .wait()
            .context("could not wait for shell")?;

        resize_guard.stop()?;

        let exit_code = u8::try_from(status.exit_code()).unwrap_or(1);

        Ok(ExitCode::from(exit_code))
    }
}
/// Prepares the shell command without starting a process.
pub fn build_shell_command(shell_path: &OsStr, login: bool) -> CommandBuilder {
    let mut command = CommandBuilder::new(shell_path);
    // Interactive mode lets the shell show a prompt and accept typed commands.
    command.arg("-i");

    if login {
        // Login mode loads startup files such as zsh's .zprofile.
        command.arg("-l")
    }

    command
}

/// Replaces stray with the shell on the existing terminal.
pub fn run_shell_directly(shell_path: &OsStr, login: bool) -> Result<ExitCode> {
    let mut command = Command::new(shell_path);
    command.arg("-i");

    if login {
        command.arg("-l");
    }

    let error = command.exec();
    Err(error).context("could not start shell directly")
}

/// Opens terminal pair that Stray and shell will use.
pub fn open_pty(size: PtySize) -> Result<PtyPair> {
    native_pty_system()
        .openpty(size)
        .context("could not open PTY")
}

/// Updates the shels terminal size and notifies its foreground program.
pub fn resize_pty(master: &dyn MasterPty, size: PtySize) -> Result<()> {
    master.resize(size).context("could not resize PTY")
}

/// Forwards window size changes while the main thread handles shell output.
pub fn start_resize_thread(
    master: Box<dyn MasterPty + Send>,
    mut signals: Signals,
) -> Result<ResizeGuard> {
    let signal_handle = signals.handle();

    let thread = thread::Builder::new()
        .name("terminal-resize".into())
        .spawn(move || {
            for _ in signals.forever() {
                let size = terminal::read_size()?;
                resize_pty(master.as_ref(), size)?;
            }

            Ok(())
        })
        .context("could not start resize thread")?;

    Ok(ResizeGuard {
        signal_handle,
        thread: Some(thread),
    })
}

/// Starts the shell on the shell's side of the PTY.
pub fn spawn_shell(pty: &PtyPair, command: CommandBuilder) -> Result<Box<dyn Child + Send + Sync>> {
    pty.slave
        .spawn_command(command)
        .context("could not start shell")
}

/// Copies shell output to the terminal until the stream ends.
pub fn forward_output(mut reader: impl Read, mut writer: impl Write) -> Result<()> {
    let mut buffer = [0u8; 8192];

    loop {
        let count = match reader.read(&mut buffer) {
            Ok(count) => count,
            // A signal can interrupt read, try again.
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("could not read shell output"),
        };

        if count == 0 {
            return Ok(());
        }

        writer
            .write_all(&buffer[..count])
            .context("could not write shell output")?;

        // Show prompts right away, even when no newline is present.
        writer.flush().context("could not flush shell output")?;
    }
}

/// Copies keyboard input to the shell until the input stream ends.
pub fn forward_input(mut reader: impl Read, mut writer: impl Write) -> Result<()> {
    std::io::copy(&mut reader, &mut writer).context("could not forward keyboard input")?;
    Ok(())
}
/// Forwards keyboard input while the main thread handles output.
pub fn start_input_thread(writer: impl Write + Send + 'static) -> Result<JoinHandle<Result<()>>> {
    thread::Builder::new()
        .name("keyboard-input".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            forward_input(stdin.lock(), writer)
        })
        .context("could not start keyboard input thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::{self, BufWriter};
    use std::sync::{Arc, Mutex};

    fn run_in_test_terminal(test_name: &str) -> bool {
        const CHILD_TEST: &str = "STRAY_TEST_SHELL_SESSION";
        if std::env::var(CHILD_TEST).as_deref() == Ok(test_name) {
            return false;
        }

        let pty = open_pty(PtySize::default()).expect("test terminal should open");
        let reader = pty.master.try_clone_reader().expect("reader should open");
        let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
        command.args(["--exact", test_name, "--nocapture"]);
        command.env(CHILD_TEST, test_name);
        let mut child = ShellGuard {
            child: spawn_shell(&pty, command).expect("isolated test should start"),
        };
        drop(pty.slave);
        let output_thread = thread::spawn(move || {
            let mut output = Vec::new();
            forward_output(reader, &mut output).expect("test output should be readable");
            output
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.child.try_wait().expect("test should be waitable") {
                break status;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "isolated test timed out"
            );
            thread::sleep(std::time::Duration::from_millis(10));
        };
        let output = output_thread.join().expect("output thread should finish");
        assert!(
            status.success(),
            "isolated test failed: {}",
            String::from_utf8_lossy(&output)
        );
        true
    }

    fn terminal_settings() -> Vec<String> {
        let output = Command::new("/bin/stty")
            .arg("-a")
            .stdin(std::process::Stdio::inherit())
            .output()
            .expect("stty should start");
        assert!(
            output.status.success(),
            "terminal settings should be readable"
        );
        // PENDIN tracks pending input retyping and can change when raw mode ends.
        String::from_utf8(output.stdout)
            .expect("stty output should be text")
            .split_whitespace()
            .filter(|word| !matches!(word.trim_end_matches(';'), "pendin" | "-pendin"))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn session_start_restores_terminal_after_spawn_failure() {
        if run_in_test_terminal(
            "wrapper::tests::session_start_restores_terminal_after_spawn_failure",
        ) {
            return;
        }
        let before = terminal_settings();
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());

        let error = match ShellSession::start(CommandBuilder::new("/dev/null/stray-shell")) {
            Ok(_) => panic!("invalid shell should not start"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "could not start shell");
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        assert_eq!(terminal_settings(), before);
    }

    #[test]
    fn session_drop_restores_terminal_after_success() {
        if run_in_test_terminal("wrapper::tests::session_drop_restores_terminal_after_success") {
            return;
        }
        let before = terminal_settings();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "exit 7"]);

        let mut session = ShellSession::start(command).expect("session should start");

        assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());
        assert_ne!(terminal_settings(), before);
        let size = session
            .master
            .get_size()
            .expect("PTY size should be readable");
        let expected = PtySize::default();
        assert_eq!((size.rows, size.cols), (expected.rows, expected.cols));
        let status = session.shell.child.wait().expect("shell should exit");
        assert_eq!(status.exit_code(), 7);

        drop(session);

        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        assert_eq!(terminal_settings(), before);
    }

    #[test]
    fn direct_shell_failure_preserves_the_os_error() {
        // Even a failed exec changes signal handling, so isolate it from other tests.
        const CHILD_TEST: &str = "STRAY_TEST_DIRECT_SHELL_FAILURE";
        if std::env::var_os(CHILD_TEST).is_none() {
            let output = Command::new(std::env::current_exe().expect("test path should exist"))
                .args([
                    "--exact",
                    "wrapper::tests::direct_shell_failure_preserves_the_os_error",
                    "--nocapture",
                ])
                .env(CHILD_TEST, "1")
                .output()
                .expect("isolated test should start");

            assert!(
                output.status.success(),
                "isolated test failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        for login in [false, true] {
            // This path cannot be executable because /dev/null is a file.
            let error = run_shell_directly(OsStr::new("/dev/null/stray-shell"), login)
                .expect_err("invalid shell path should fail");

            assert_eq!(error.to_string(), "could not start shell directly");
            assert_eq!(
                error.downcast_ref::<io::Error>().unwrap().kind(),
                ErrorKind::NotADirectory
            );
        }
    }

    #[test]
    fn resize_thread_stops_without_a_signal_and_can_stop_twice() {
        let pty = open_pty(PtySize::default()).expect("PTY should open");
        // No subscriptions keep this test separate from other signal tests.
        let signals = Signals::new([] as [i32; 0]).expect("listener should open");
        let signal_handle = signals.handle();
        let mut guard =
            start_resize_thread(pty.master, signals).expect("resize thread should start");

        guard.stop().expect("resize thread should stop");

        assert!(signal_handle.is_closed());
        assert!(guard.thread.is_none());
        guard.stop().expect("stopping twice should be safe");
    }

    #[test]
    fn resize_guard_drop_closes_listener_and_waits_for_thread() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let mut signals = Signals::new([] as [i32; 0]).expect("listener should open");
        let signal_handle = signals.handle();
        let finished = Arc::new(AtomicBool::new(false));
        let thread_finished = Arc::clone(&finished);
        let thread = thread::spawn(move || {
            for _ in signals.forever() {}
            thread_finished.store(true, Ordering::SeqCst);
            Ok(())
        });
        let guard = ResizeGuard {
            signal_handle: signal_handle.clone(),
            thread: Some(thread),
        };

        drop(guard);

        assert!(signal_handle.is_closed());
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn resize_guard_stop_returns_thread_error() {
        let signals = Signals::new([] as [i32; 0]).expect("listener should open");
        let thread =
            thread::spawn(|| Err(io::Error::new(ErrorKind::BrokenPipe, "PTY closed").into()));
        let mut guard = ResizeGuard {
            signal_handle: signals.handle(),
            thread: Some(thread),
        };

        let error = guard.stop().expect_err("thread error should reach caller");

        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            ErrorKind::BrokenPipe
        );
        assert!(guard.signal_handle.is_closed());
        assert!(guard.thread.is_none());
        guard.stop().expect("cleanup after an error should be safe");
    }

    #[test]
    fn resize_guard_stop_reports_thread_panic() {
        let signals = Signals::new([] as [i32; 0]).expect("listener should open");
        let thread = thread::spawn(|| panic!("test resize thread panic"));
        let mut guard = ResizeGuard {
            signal_handle: signals.handle(),
            thread: Some(thread),
        };

        let error = guard
            .stop()
            .expect_err("thread panic should become an error");

        assert_eq!(error.to_string(), "resize thread panicked");
        assert!(guard.thread.is_none());
        guard.stop().expect("cleanup after a panic should be safe");
    }

    #[derive(Clone, Debug)]
    struct TestChild {
        finished: bool,
        poll_fails: bool,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl portable_pty::ChildKiller for TestChild {
        fn kill(&mut self) -> io::Result<()> {
            self.calls.lock().unwrap().push("kill");
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            Box::new(self.clone())
        }
    }

    impl Child for TestChild {
        fn try_wait(&mut self) -> io::Result<Option<portable_pty::ExitStatus>> {
            self.calls.lock().unwrap().push("try_wait");
            if self.poll_fails {
                return Err(io::Error::other("could not check child"));
            }
            Ok(self
                .finished
                .then(|| portable_pty::ExitStatus::with_exit_code(7)))
        }

        fn wait(&mut self) -> io::Result<portable_pty::ExitStatus> {
            self.calls.lock().unwrap().push("wait");
            Ok(portable_pty::ExitStatus::with_exit_code(7))
        }

        fn process_id(&self) -> Option<u32> {
            None
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    #[test]
    fn shell_guard_stops_and_collects_a_running_shell() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let guard = ShellGuard {
            child: Box::new(TestChild {
                finished: false,
                poll_fails: false,
                calls: Arc::clone(&calls),
            }),
        };

        drop(guard);

        assert_eq!(*calls.lock().unwrap(), ["try_wait", "kill", "wait"]);
    }

    #[test]
    fn shell_guard_leaves_a_finished_shell_alone() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let guard = ShellGuard {
            child: Box::new(TestChild {
                finished: true,
                poll_fails: false,
                calls: Arc::clone(&calls),
            }),
        };

        drop(guard);

        assert_eq!(*calls.lock().unwrap(), ["try_wait"]);
    }

    #[test]
    fn shell_guard_attempts_cleanup_when_checking_the_shell_fails() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let guard = ShellGuard {
            child: Box::new(TestChild {
                finished: false,
                poll_fails: true,
                calls: Arc::clone(&calls),
            }),
        };

        drop(guard);

        assert_eq!(*calls.lock().unwrap(), ["try_wait", "kill", "wait"]);
    }

    #[test]
    fn input_preserves_text_and_control_keys() {
        // Include Enter, Tab, Backspace, Ctrl+C, and an arrow key.
        let input = b"echo hello\r\t\x7f\x03\x1b[A";
        let mut output = Vec::new();

        forward_input(&input[..], &mut output).expect("keyboard input should copy");

        assert_eq!(output, input);
    }

    #[test]
    fn input_reports_a_write_failure() {
        let mut output = [0u8; 2];

        let error = forward_input(&b"echo hello\r"[..], &mut output[..])
            .expect_err("writing past the available space should fail");

        assert_eq!(error.to_string(), "could not forward keyboard input");
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            ErrorKind::WriteZero
        );
    }

    #[test]
    fn shell_starts_and_returns_its_exit_code() {
        let pty = open_pty(PtySize::default()).expect("PTY should open");
        let mut command = CommandBuilder::new("/bin/sh");
        // Exit immediately so the test never waits for keyboard input.
        command.args(["-c", "exit 7"]);

        let mut child = spawn_shell(&pty, command).expect("shell should start");
        drop(pty.slave);
        let status = child.wait().expect("shell should finish");

        assert_eq!(status.exit_code(), 7);
    }

    #[test]
    fn missing_shell_returns_an_error() {
        let pty = open_pty(PtySize::default()).expect("PTY should open");
        // A regular file cannot contain another executable path.
        let command = CommandBuilder::new("/dev/null/stray-shell");

        let error = spawn_shell(&pty, command).expect_err("missing shell should fail");

        assert_eq!(error.to_string(), "could not start shell");
    }

    #[test]
    fn output_preserves_bytes_and_flushes_the_final_prompt() {
        // Use every byte value and enough data to require several reads.
        let mut input: Vec<u8> = (0..=255).cycle().take(20_000).collect();
        input.extend_from_slice(b"\x1b[32mshell> ");
        let mut output = Vec::new();
        let mut writer = BufWriter::with_capacity(1024, &mut output);

        forward_output(input.as_slice(), &mut writer).expect("output should copy");

        assert!(writer.buffer().is_empty(), "the prompt should be flushed");
        drop(writer);
        assert_eq!(output, input);
    }

    #[test]
    fn output_retries_an_interrupted_read() {
        struct InterruptedOnce {
            interrupted: bool,
            remaining: &'static [u8],
        }

        impl Read for InterruptedOnce {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(ErrorKind::Interrupted.into());
                }
                self.remaining.read(buffer)
            }
        }

        let reader = InterruptedOnce {
            interrupted: false,
            remaining: b"shell> ",
        };
        let mut output = Vec::new();

        forward_output(reader, &mut output).expect("interrupted read should retry");

        assert_eq!(output, b"shell> ");
    }

    #[test]
    fn output_reports_a_write_failure() {
        // A fixed slice runs out of space instead of growing like a Vec.
        let mut output = [0u8; 2];

        let error = forward_output(&b"shell> "[..], &mut output[..])
            .expect_err("writing past the available space should fail");

        assert_eq!(error.to_string(), "could not write shell output");
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            ErrorKind::WriteZero
        );
    }

    #[test]
    fn pty_uses_requested_size() {
        for (rows, cols) in [(24, 80), (40, 120), (10, 30)] {
            let size = PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            };

            // Read the size from a real PTY to check what the OS received.
            let pty = open_pty(size).expect("PTY should open");
            let actual_size = pty.master.get_size().expect("PTY size should be readable");

            assert_eq!(actual_size.rows, rows);
            assert_eq!(actual_size.cols, cols);
            assert_eq!(actual_size.pixel_width, 0);
            assert_eq!(actual_size.pixel_height, 0);
        }
    }

    #[test]
    fn pty_resizes_larger_and_smaller() {
        let pty = open_pty(PtySize::default()).expect("PTY should open");

        for (rows, cols) in [(40, 120), (10, 30), (24, 80)] {
            let size = PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            };

            resize_pty(pty.master.as_ref(), size).expect("PTY should resize");
            let actual_size = pty.master.get_size().expect("PTY size should be readable");

            assert_eq!(actual_size.rows, rows);
            assert_eq!(actual_size.cols, cols);
            assert_eq!(actual_size.pixel_width, 0);
            assert_eq!(actual_size.pixel_height, 0);
        }
    }

    #[test]
    fn shell_command_is_interactive_without_login() {
        for shell_path in ["/bin/zsh", "/bin/bash"] {
            let command = build_shell_command(OsStr::new(shell_path), false);
            let expected = vec![OsString::from(shell_path), OsString::from("-i")];

            assert_eq!(command.get_argv(), &expected);
        }
    }

    #[test]
    fn login_shell_command_is_interactive_and_has_login_flag() {
        for shell_path in ["/bin/zsh", "/bin/bash"] {
            let command = build_shell_command(OsStr::new(shell_path), true);
            let expected = vec![
                OsString::from(shell_path),
                OsString::from("-i"),
                OsString::from("-l"),
            ];

            assert_eq!(command.get_argv(), &expected);
        }
    }
}
