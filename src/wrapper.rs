use crate::terminal;
use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtyPair, PtySize, native_pty_system};
use signal_hook::iterator::{Handle, Signals};
use std::ffi::OsStr;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};
use std::sync::mpsc::{Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use stray_cat::bar;

pub const SHELL_MARKER: &str = "STRAY_TTY";

enum SessionEvent {
    Output(Vec<u8>),
    InputFinished(Result<()>),
    OutputFinished(Result<()>),
    Resized,
    ResizeFailed(anyhow::Error),
    Terminated(i32),
}

#[derive(Debug)]
pub struct TerminationSignal(pub i32);

impl std::fmt::Display for TerminationSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "session stopped by signal {}", self.0)
    }
}

impl std::error::Error for TerminationSignal {}

/// Converts an unwinding panic into an error after the closure's guards are dropped.
pub fn catch_panic<T>(message: &'static str, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .map_err(|_| anyhow::anyhow!(message))?
}

struct OutputSender {
    events: SyncSender<SessionEvent>,
}

impl Write for OutputSender {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.events
            .send(SessionEvent::Output(buffer.to_vec()))
            .map_err(|_| std::io::Error::new(ErrorKind::BrokenPipe, "session has ended"))?;

        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // The main thread flushes the terminal after receiving each output event.
        Ok(())
    }
}

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
    output: bar::Output<std::io::Stdout>,
    _raw_mode: terminal::RawModeGuard,
}

impl ShellSession {
    #[cfg(test)]
    pub fn start(command: CommandBuilder) -> Result<Self> {
        Self::start_with_bar(command, false, None)
    }

    pub fn start_with_bar(
        mut command: CommandBuilder,
        images_supported: bool,
        cursor: Option<(u16, u16)>,
    ) -> Result<Self> {
        let signals = terminal::watch_signals()?;
        let size = terminal::read_size()?;
        let pty = open_pty(bar::shell_size(size, images_supported))?;
        let tty_name = pty
            .master
            .tty_name()
            .context("could not read shell PTY name")?;
        command.env(SHELL_MARKER, tty_name);

        let reader = pty
            .master
            .try_clone_reader()
            .context("could not open PTY reader")?;

        let writer = pty
            .master
            .take_writer()
            .context("could not open PTY writer")?;

        let raw_mode = terminal::RawModeGuard::enable()?;
        let output = bar::Output::new(std::io::stdout(), size, images_supported, cursor)
            .context("could not reserve bar rows")?;

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
            output,
            _raw_mode: raw_mode,
        })
    }

    #[cfg(test)]
    pub fn run(self) -> Result<ExitCode> {
        self.run_with_input(Vec::new())
    }

    pub fn run_with_input(mut self, input: Vec<u8>) -> Result<ExitCode> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        let mut resize_guard = start_resize_thread(self.signals, sender.clone())?;

        forward_session(
            self.reader,
            self.writer,
            input,
            sender,
            receiver,
            self.output,
            |output| {
                let size = output.resize(terminal::read_size()?)?;
                resize_pty(self.master.as_ref(), size)
            },
        )?;

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
    let error = build_direct_shell_command(shell_path, login).exec();
    Err(error).context("could not start shell directly")
}

fn build_direct_shell_command(shell_path: &OsStr, login: bool) -> Command {
    let mut command = Command::new(shell_path);
    command.arg("-i");

    if login {
        command.arg("-l");
    }

    if let Some(tty_name) = terminal::tty_name() {
        command.env(SHELL_MARKER, tty_name);
    } else {
        command.env_remove(SHELL_MARKER);
    }

    command
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

/// Sends window changes and shutdown requests to the session loop.
fn start_resize_thread(
    mut signals: Signals,
    events: SyncSender<SessionEvent>,
) -> Result<ResizeGuard> {
    let signal_handle = signals.handle();

    let thread = thread::Builder::new()
        .name("terminal-resize".into())
        .spawn(move || {
            let result = catch_panic("resize thread panicked", || {
                for signal in signals.forever() {
                    if signal == signal_hook::consts::SIGWINCH {
                        if events.send(SessionEvent::Resized).is_err() {
                            break;
                        }
                    } else {
                        let _ = events.send(SessionEvent::Terminated(signal));
                        break;
                    }
                }
                Ok(())
            });
            if let Err(error) = result {
                let _ = events.send(SessionEvent::ResizeFailed(error));
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

fn forward_session(
    reader: impl Read + Send + 'static,
    writer: impl Write + Send + 'static,
    input: Vec<u8>,
    sender: SyncSender<SessionEvent>,
    receiver: Receiver<SessionEvent>,
    output: bar::Output<std::io::Stdout>,
    resize: impl FnMut(&mut bar::Output<std::io::Stdout>) -> Result<()>,
) -> Result<()> {
    // Owning the receiver here closes the queue before resize cleanup on any error.
    let _input_thread = start_input_thread(writer, input, sender.clone())?;
    let output_thread = start_output_thread(reader, sender)?;
    forward_session_events(receiver, output, resize)?;
    output_thread
        .join()
        .map_err(|_| anyhow::anyhow!("shell output thread panicked"))
}

/// Handles session events until shell output ends or a worker fails.
fn forward_session_events<W: Write>(
    receiver: Receiver<SessionEvent>,
    mut output: W,
    mut resize: impl FnMut(&mut W) -> Result<()>,
) -> Result<()> {
    loop {
        let event = receiver
            .recv()
            .context("session event channel closed unexpectedly")?;

        match event {
            SessionEvent::Output(bytes) => {
                output
                    .write_all(&bytes)
                    .context("could not write shell output")?;
                output.flush().context("could not flush shell output")?;
            }
            SessionEvent::InputFinished(result) => {
                result?;
            }
            SessionEvent::OutputFinished(result) => {
                return result;
            }
            SessionEvent::Resized => {
                catch_panic("resize handler panicked", || resize(&mut output))?
            }
            SessionEvent::ResizeFailed(error) => return Err(error),
            SessionEvent::Terminated(signal) => return Err(TerminationSignal(signal).into()),
        }
    }
}

/// Forwards keys directly and reports when input forwarding ends.
fn start_input_thread(
    mut writer: impl Write + Send + 'static,
    input: Vec<u8>,
    events: SyncSender<SessionEvent>,
) -> Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("keyboard-input".into())
        .spawn(move || {
            let result = catch_panic("keyboard input thread panicked", || {
                writer
                    .write_all(&input)
                    .context("could not forward input from image check")?;
                let stdin = std::io::stdin();
                forward_input(stdin.lock(), writer)
            });

            // A closed receiver means the session has already ended.
            let _ = events.send(SessionEvent::InputFinished(result));
        })
        .context("could not start keyboard input thread")
}

fn start_output_thread(
    reader: impl Read + Send + 'static,
    events: SyncSender<SessionEvent>,
) -> Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("shell-output".into())
        .spawn(move || {
            let result = catch_panic("shell output thread panicked", || {
                let writer = OutputSender {
                    events: events.clone(),
                };
                forward_output(reader, writer)
            });

            // A closed receiver means the session has already ended.
            let _ = events.send(SessionEvent::OutputFinished(result));
        })
        .context("could not start shell output thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::{self, BufWriter};
    use std::sync::{Arc, Mutex};

    #[test]
    fn session_marker_matches_the_inner_terminal_and_replaces_a_stale_marker() {
        if run_in_test_terminal(
            "wrapper::tests::session_marker_matches_the_inner_terminal_and_replaces_a_stale_marker",
        ) {
            return;
        }
        let outer_terminal = terminal::tty_name().unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "printf '%s\\n' \"$STRAY_TTY\"; /usr/bin/tty"]);
        command.env(SHELL_MARKER, "/dev/stale-terminal");
        let mut session = ShellSession::start(command).unwrap();
        let inner_terminal = session.master.tty_name().unwrap();
        let mut output = Vec::new();

        forward_output(&mut session.reader, &mut output).unwrap();
        let status = session.shell.child.wait().unwrap();
        drop(session);

        assert!(status.success());
        assert_ne!(inner_terminal.as_os_str(), outer_terminal);
        let text = String::from_utf8(output).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines, vec![inner_terminal.to_str().unwrap(); 2]);
        assert_eq!(terminal::tty_name().unwrap(), outer_terminal);
    }

    #[test]
    fn direct_shell_marker_matches_the_existing_terminal() {
        if run_in_test_terminal("wrapper::tests::direct_shell_marker_matches_the_existing_terminal")
        {
            return;
        }
        let terminal_name = terminal::tty_name().unwrap();
        let mut command = build_direct_shell_command(OsStr::new("/bin/sh"), false);
        command.args(["-c", "printf '%s\\n' \"$STRAY_TTY\"; /usr/bin/tty"]);
        command.env("ENV", "/dev/null");
        command.stdin(std::process::Stdio::inherit());

        let output = command.output().unwrap();

        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines, vec![terminal_name.to_str().unwrap(); 2]);
    }

    #[test]
    fn typed_ctrl_c_reaches_the_inner_shell() {
        if run_in_test_terminal("wrapper::tests::typed_ctrl_c_reaches_the_inner_shell") {
            return;
        }
        let before = terminal_settings();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "trap 'exit 7' INT; printf READY; read value"]);
        let mut session = ShellSession::start(command).unwrap();
        let mut ready = [0u8; 5];
        session.reader.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"READY");

        session.writer.write_all(b"\x03").unwrap();
        session.writer.flush().unwrap();
        let status = session.run().unwrap();

        assert_eq!(status, ExitCode::from(7));
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        assert_eq!(terminal_settings(), before);
    }

    #[test]
    fn termination_signals_stop_the_shell_and_restore_the_terminal() {
        if run_in_test_terminal(
            "wrapper::tests::termination_signals_stop_the_shell_and_restore_the_terminal",
        ) {
            return;
        }
        use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
        let before = terminal_settings();

        for signal in [SIGHUP, SIGINT, SIGQUIT, SIGTERM] {
            let mut command = CommandBuilder::new("/bin/sh");
            command.args(["-c", "read value"]);
            let session = ShellSession::start(command).unwrap();
            let shell_pid = session.shell.child.process_id().unwrap();
            assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());

            // Queue the signal after registration, before the event loop starts.
            signal_hook::low_level::raise(signal).unwrap();
            let error = session.run().unwrap_err();

            assert_eq!(error.downcast_ref::<TerminationSignal>().unwrap().0, signal);
            assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
            assert_eq!(terminal_settings(), before);
            let alive = Command::new("/bin/kill")
                .args(["-0", &shell_pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(!alive.success(), "the shell must be stopped and collected");
        }
    }

    #[test]
    fn termination_closes_the_queue_before_worker_cleanup() {
        let signal = signal_hook::consts::SIGTERM;
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        sender.send(SessionEvent::Terminated(signal)).unwrap();
        sender
            .send(SessionEvent::Output(b"too late".to_vec()))
            .unwrap();
        let mut output = Vec::new();

        let error = forward_session_events(receiver, &mut output, |_| Ok(())).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("session stopped by signal {signal}")
        );
        assert_eq!(error.downcast_ref::<TerminationSignal>().unwrap().0, signal);
        assert!(output.is_empty());
        assert!(sender.send(SessionEvent::Output(vec![])).is_err());
    }

    #[test]
    fn panic_boundary_preserves_success_and_regular_errors() {
        assert_eq!(catch_panic("unexpected panic", || Ok(7)).unwrap(), 7);
        let error = catch_panic::<()>("unexpected panic", || {
            Err(io::Error::new(ErrorKind::BrokenPipe, "test failure").into())
        })
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn session_panic_restores_terminal_before_returning_an_error() {
        if run_in_test_terminal(
            "wrapper::tests::session_panic_restores_terminal_before_returning_an_error",
        ) {
            return;
        }
        let before = terminal_settings();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "read value"]);

        let error = catch_panic::<()>("session panicked", || {
            let _session = ShellSession::start(command).unwrap();
            assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());
            panic!("test session panic");
        })
        .unwrap_err();

        assert_eq!(error.to_string(), "session panicked");
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        assert_eq!(terminal_settings(), before);
    }

    #[test]
    fn output_panic_reports_failure_while_other_senders_remain_open() {
        struct PanickingReader;
        impl Read for PanickingReader {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("test output panic");
            }
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let thread = start_output_thread(PanickingReader, sender.clone()).unwrap();

        let event = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();

        match event {
            SessionEvent::OutputFinished(Err(error)) => {
                assert_eq!(error.to_string(), "shell output thread panicked");
            }
            _ => panic!("expected an output panic error"),
        }
        thread.join().unwrap();
        assert!(matches!(
            receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        drop(sender);
    }

    #[test]
    fn resize_panic_closes_the_session_before_cleanup() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        sender.send(SessionEvent::Resized).unwrap();
        sender
            .send(SessionEvent::Output(b"too late".to_vec()))
            .unwrap();
        let mut output = Vec::new();

        let error = forward_session_events(receiver, &mut output, |_| {
            panic!("test resize panic");
        })
        .unwrap_err();

        assert_eq!(error.to_string(), "resize handler panicked");
        assert!(output.is_empty());
        assert!(sender.send(SessionEvent::Resized).is_err());
    }

    #[test]
    fn session_resizes_between_flushed_output_events() {
        struct Output<'a>(&'a std::cell::RefCell<Vec<u8>>);
        impl Write for Output<'_> {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.borrow_mut().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.0.borrow_mut().push(b'|');
                Ok(())
            }
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        sender
            .send(SessionEvent::Output(b"before".to_vec()))
            .unwrap();
        sender.send(SessionEvent::Resized).unwrap();
        sender
            .send(SessionEvent::Output(b"after".to_vec()))
            .unwrap();
        sender.send(SessionEvent::OutputFinished(Ok(()))).unwrap();
        let calls = std::cell::RefCell::new(Vec::new());
        let session_thread = thread::current().id();

        forward_session_events(receiver, Output(&calls), |_| {
            assert_eq!(thread::current().id(), session_thread);
            calls.borrow_mut().extend_from_slice(b"resize|");
            Ok(())
        })
        .unwrap();

        assert_eq!(*calls.borrow(), b"before|resize|after|");
    }

    #[test]
    fn session_events_flush_output_after_input_finishes() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        sender
            .send(SessionEvent::Output(b"first\n".to_vec()))
            .unwrap();
        sender.send(SessionEvent::InputFinished(Ok(()))).unwrap();
        sender
            .send(SessionEvent::Output(b"\x1b[32mshell> ".to_vec()))
            .unwrap();
        sender.send(SessionEvent::OutputFinished(Ok(()))).unwrap();
        let mut bytes = Vec::new();
        let mut output = BufWriter::new(&mut bytes);

        forward_session_events(receiver, &mut output, |_| Ok(())).unwrap();

        assert!(
            output.buffer().is_empty(),
            "the final prompt must be flushed"
        );
        drop(output);
        assert_eq!(bytes, b"first\n\x1b[32mshell> ");
        assert!(sender.send(SessionEvent::Output(vec![])).is_err());
    }

    #[test]
    fn session_events_return_worker_errors_and_close_the_receiver() {
        for worker in ["input", "output", "resize", "resize handler"] {
            let (sender, receiver) = std::sync::mpsc::sync_channel(3);
            let error = io::Error::new(ErrorKind::PermissionDenied, "test failure").into();
            let event = match worker {
                "input" => SessionEvent::InputFinished(Err(error)),
                "output" => SessionEvent::OutputFinished(Err(error)),
                "resize handler" => SessionEvent::Resized,
                _ => SessionEvent::ResizeFailed(error),
            };
            sender.send(event).unwrap();
            sender
                .send(SessionEvent::Output(b"too late".to_vec()))
                .unwrap();
            sender.send(SessionEvent::OutputFinished(Ok(()))).unwrap();
            let mut output = Vec::new();

            let error = forward_session_events(receiver, &mut output, |_| {
                Err(io::Error::new(ErrorKind::PermissionDenied, "resize failed").into())
            })
            .unwrap_err();

            assert_eq!(
                error.downcast_ref::<io::Error>().unwrap().kind(),
                ErrorKind::PermissionDenied
            );
            assert!(
                output.is_empty(),
                "output after the failure must not be written"
            );
            assert!(sender.send(SessionEvent::Output(vec![])).is_err());
        }
    }

    #[test]
    fn session_events_require_output_completion_before_disconnect() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender.send(SessionEvent::InputFinished(Ok(()))).unwrap();
        drop(sender);

        let error = forward_session_events(receiver, io::sink(), |_| Ok(())).unwrap_err();

        assert_eq!(
            error.to_string(),
            "session event channel closed unexpectedly"
        );
    }

    #[test]
    fn session_events_report_terminal_write_and_flush_failures() {
        struct FailingWriter {
            fail_on_flush: bool,
        }

        impl Write for FailingWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.fail_on_flush {
                    Ok(bytes.len())
                } else {
                    Err(ErrorKind::BrokenPipe.into())
                }
            }

            fn flush(&mut self) -> io::Result<()> {
                Err(ErrorKind::BrokenPipe.into())
            }
        }

        for fail_on_flush in [false, true] {
            let (sender, receiver) = std::sync::mpsc::sync_channel(2);
            sender
                .send(SessionEvent::Output(b"shell> ".to_vec()))
                .unwrap();
            sender.send(SessionEvent::OutputFinished(Ok(()))).unwrap();

            let error =
                forward_session_events(receiver, FailingWriter { fail_on_flush }, |_| Ok(()))
                    .unwrap_err();

            let expected = if fail_on_flush {
                "could not flush shell output"
            } else {
                "could not write shell output"
            };
            assert_eq!(error.to_string(), expected);
            assert_eq!(
                error.downcast_ref::<io::Error>().unwrap().kind(),
                ErrorKind::BrokenPipe
            );
            assert!(sender.send(SessionEvent::Output(vec![])).is_err());
        }
    }

    #[test]
    fn output_thread_preserves_bytes_before_reporting_completion() {
        let expected: Vec<u8> = (0..=255).cycle().take(25_000).collect();
        let reader = io::Cursor::new(expected.clone());
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let thread = start_output_thread(reader, sender).expect("output thread should start");
        let mut received = Vec::new();

        loop {
            let event = receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("output thread should report an event");
            match event {
                SessionEvent::Output(bytes) => received.extend_from_slice(&bytes),
                SessionEvent::OutputFinished(result) => {
                    result.expect("output should finish successfully");
                    break;
                }
                SessionEvent::InputFinished(_) => panic!("output thread sent an input event"),
                SessionEvent::Resized | SessionEvent::ResizeFailed(_) => {
                    panic!("output thread sent a resize event")
                }
                SessionEvent::Terminated(_) => panic!("output thread sent a termination event"),
            }
        }

        thread.join().expect("output thread should not panic");
        assert_eq!(received, expected);
        assert!(matches!(
            receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn output_thread_reports_read_errors() {
        struct FailingReader;

        impl Read for FailingReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    ErrorKind::PermissionDenied,
                    "test read failure",
                ))
            }
        }

        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let thread =
            start_output_thread(FailingReader, sender).expect("output thread should start");
        let event = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("output thread should report the failure");

        match event {
            SessionEvent::OutputFinished(Err(error)) => {
                assert_eq!(error.to_string(), "could not read shell output");
                assert_eq!(
                    error.downcast_ref::<io::Error>().unwrap().kind(),
                    ErrorKind::PermissionDenied
                );
            }
            _ => panic!("expected a failed output completion event"),
        }
        thread.join().expect("output thread should not panic");
    }

    #[test]
    fn output_sender_reports_a_closed_receiver() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let mut writer = OutputSender { events: sender };
        drop(receiver);

        let error = writer
            .write(b"shell> ")
            .expect_err("closed queue should fail");

        assert_eq!(error.kind(), ErrorKind::BrokenPipe);
        assert_eq!(error.to_string(), "session has ended");
    }

    #[test]
    fn output_thread_exits_when_the_receiver_closes() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let reader = io::Cursor::new(vec![b'x'; 25_000]);
        let thread = start_output_thread(reader, sender).expect("output thread should start");
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("first output event should arrive");

        drop(receiver);

        thread
            .join()
            .expect("closing the queue should stop output cleanly");
    }

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
        // No subscriptions keep this test separate from other signal tests.
        let signals = Signals::new([] as [i32; 0]).expect("listener should open");
        let signal_handle = signals.handle();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let mut guard = start_resize_thread(signals, sender).expect("resize thread should start");

        guard.stop().expect("resize thread should stop");

        assert!(signal_handle.is_closed());
        assert!(guard.thread.is_none());
        guard.stop().expect("stopping twice should be safe");
        assert!(matches!(
            receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn resize_signal_reaches_the_session() {
        if run_in_test_terminal("wrapper::tests::resize_signal_reaches_the_session") {
            return;
        }
        let signals = terminal::watch_signals().unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let mut guard = start_resize_thread(signals, sender).unwrap();
        signal_hook::low_level::raise(signal_hook::consts::SIGWINCH).unwrap();

        let event = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();

        assert!(matches!(event, SessionEvent::Resized));
        guard.stop().unwrap();
    }

    #[test]
    fn closing_the_session_releases_a_resize_worker_with_a_full_queue() {
        if run_in_test_terminal(
            "wrapper::tests::closing_the_session_releases_a_resize_worker_with_a_full_queue",
        ) {
            return;
        }
        let signals = terminal::watch_signals().unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender.send(SessionEvent::OutputFinished(Ok(()))).unwrap();
        signal_hook::low_level::raise(signal_hook::consts::SIGWINCH).unwrap();
        let mut guard = start_resize_thread(signals, sender).unwrap();

        forward_session_events(receiver, io::sink(), |_| Ok(())).unwrap();
        guard.stop().unwrap();

        assert!(guard.thread.is_none());
        assert!(guard.signal_handle.is_closed());
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
