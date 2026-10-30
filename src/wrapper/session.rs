//! The shell session: starts the shell in a PTY and runs the loop that passes data along.

use super::launch::{ShellGuard, open_pty, resize_pty, spawn_shell};
use super::workers::{start_input_thread, start_output_thread, start_resize_thread};
use super::{SHELL_MARKER, TerminationSignal, catch_panic};
use crate::terminal;
use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty};
use signal_hook::iterator::Signals;
use std::io::{Read, Stdout, Write};
use std::process::ExitCode;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use stray_cat::bar;

/// How many events can wait in the queue before workers pause.
const EVENT_QUEUE_SIZE: usize = 16;

/// Exit code used when the shell's own code does not fit in a byte.
const FALLBACK_EXIT_CODE: u8 = 1;

/// Messages from the worker threads to the session loop.
pub enum SessionEvent {
    /// Bytes the shell printed.
    Output(Vec<u8>),
    /// Keyboard input ended, or failed.
    InputFinished(Result<()>),
    /// Shell output ended, which ends the session.
    OutputFinished(Result<()>),
    /// The terminal window changed size.
    Resized,
    /// The signal thread failed.
    ResizeFailed(anyhow::Error),
    /// A shutdown signal such as SIGTERM arrived.
    Terminated(i32),
}

/// A running shell, with the terminal in raw mode until the session is dropped.
pub struct ShellSession {
    shell: ShellGuard,
    master: Box<dyn MasterPty + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    signals: Signals,
    output: bar::Output<Stdout>,
    _raw_mode: terminal::RawModeGuard,
}

impl ShellSession {
    #[cfg(test)]
    pub fn start(command: CommandBuilder) -> Result<Self> {
        Self::start_with_bar(command, bar::Setup::default())
    }

    /// Starts the shell in a new PTY, leaving room for the bar when images are supported.
    pub fn start_with_bar(mut command: CommandBuilder, setup: bar::Setup) -> Result<Self> {
        // Watch signals before raw mode, so a shutdown signal also takes the cleanup path.
        let signals = terminal::watch_signals()?;
        let size = terminal::read_size()?;
        let pty = open_pty(bar::shell_size(size, setup.images_supported))?;
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
        let output = bar::Output::new(std::io::stdout(), size, setup)
            .context("could not reserve bar rows")?;

        let shell = ShellGuard {
            child: spawn_shell(&pty, command)?,
        };
        // Only the shell should hold its side of the PTY, so output ends when the shell exits.
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

    /// Runs the session until the shell exits, and returns the shell's exit code.
    ///
    /// `early_input` holds keys typed during the image check, which reach the shell first.
    pub fn run_with_input(mut self, early_input: Vec<u8>) -> Result<ExitCode> {
        let (sender, receiver) = sync_channel(EVENT_QUEUE_SIZE);
        // Each worker needs its own sender. Cloning a sender does not copy any events.
        let mut resize_guard = start_resize_thread(self.signals, sender.clone())?;
        let master = self.master;

        forward_session(
            self.reader,
            self.writer,
            early_input,
            sender,
            receiver,
            self.output,
            |output| {
                let size = output.resize(terminal::read_size()?)?;
                resize_pty(master.as_ref(), size)
            },
        )?;

        let status = self
            .shell
            .child
            .wait()
            .context("could not wait for shell")?;
        resize_guard.stop()?;

        let exit_code = u8::try_from(status.exit_code()).unwrap_or(FALLBACK_EXIT_CODE);
        Ok(ExitCode::from(exit_code))
    }
}

/// Starts the input and output threads, then runs the session loop.
fn forward_session(
    reader: impl Read + Send + 'static,
    writer: impl Write + Send + 'static,
    early_input: Vec<u8>,
    sender: SyncSender<SessionEvent>,
    receiver: Receiver<SessionEvent>,
    output: bar::Output<Stdout>,
    resize: impl FnMut(&mut bar::Output<Stdout>) -> Result<()>,
) -> Result<()> {
    // The input thread may stay blocked on stdin, so it is not joined.
    let _input_thread = start_input_thread(writer, early_input, sender.clone())?;
    let output_thread = start_output_thread(reader, sender)?;
    // Owning the receiver here closes the queue before resize cleanup on any error.
    forward_session_events(receiver, output, resize)?;
    output_thread
        .join()
        .map_err(|_| anyhow::anyhow!("shell output thread panicked"))
}

/// Handles session events until shell output ends or a worker fails.
pub fn forward_session_events<W: Write>(
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
            SessionEvent::InputFinished(result) => result?,
            SessionEvent::OutputFinished(result) => return result,
            SessionEvent::Resized => {
                catch_panic("resize handler panicked", || resize(&mut output))?;
            }
            SessionEvent::ResizeFailed(error) => return Err(error),
            SessionEvent::Terminated(signal) => return Err(TerminationSignal(signal).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_terminal::{run_in_test_terminal, terminal_settings, test_name};
    use super::super::workers::forward_output;
    use super::*;
    use portable_pty::PtySize;
    use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
    use std::cell::RefCell;
    use std::ffi::OsStr;
    use std::io::{self, BufWriter, ErrorKind};
    use std::process::{Command, Stdio};
    use std::thread;

    /// Prints the marker, then the real terminal name, so the two can be compared.
    const PRINT_MARKER_AND_TTY: &str = "printf '%s\\n' \"$STRAY_TTY\"; /usr/bin/tty";

    /// Output that arrives after the session should have stopped.
    const TOO_LATE: &[u8] = b"too late";

    fn shell_command(script: &str) -> CommandBuilder {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    fn raw_mode_enabled() -> Result<bool> {
        Ok(crossterm::terminal::is_raw_mode_enabled()?)
    }

    /// Checks that the session closed its queue, so no worker can send more events.
    fn queue_is_closed(sender: &SyncSender<SessionEvent>) -> bool {
        sender.send(SessionEvent::Output(Vec::new())).is_err()
    }

    #[test]
    fn session_marker_matches_the_inner_terminal_and_replaces_a_stale_marker() -> Result<()> {
        if run_in_test_terminal(test_name!(
            session_marker_matches_the_inner_terminal_and_replaces_a_stale_marker
        ))? {
            return Ok(());
        }
        let outer_terminal = terminal::tty_name().context("test should have a terminal")?;
        let mut command = shell_command(PRINT_MARKER_AND_TTY);
        command.env(SHELL_MARKER, "/dev/stale-terminal");
        let mut session = ShellSession::start(command)?;
        let inner_terminal = session
            .master
            .tty_name()
            .context("PTY should have a name")?;
        let mut output = Vec::new();

        forward_output(&mut session.reader, &mut output)?;
        let status = session.shell.child.wait()?;
        drop(session);

        assert!(status.success());
        assert_ne!(inner_terminal.as_os_str(), outer_terminal);
        let text = String::from_utf8(output)?;
        let lines: Vec<_> = text.lines().map(OsStr::new).collect();
        assert_eq!(lines, vec![inner_terminal.as_os_str(); 2]);
        assert_eq!(terminal::tty_name(), Some(outer_terminal));
        Ok(())
    }

    #[test]
    fn typed_ctrl_c_reaches_the_inner_shell() -> Result<()> {
        if run_in_test_terminal(test_name!(typed_ctrl_c_reaches_the_inner_shell))? {
            return Ok(());
        }
        let before = terminal_settings()?;
        let command = shell_command("trap 'exit 7' INT; printf READY; read value");
        let mut session = ShellSession::start(command)?;
        let mut ready = [0u8; 5];
        session.reader.read_exact(&mut ready)?;
        assert_eq!(&ready, b"READY");

        session.writer.write_all(b"\x03")?;
        session.writer.flush()?;
        let status = session.run()?;

        assert_eq!(status, ExitCode::from(7));
        assert!(!raw_mode_enabled()?);
        assert_eq!(terminal_settings()?, before);
        Ok(())
    }

    #[test]
    fn termination_signals_stop_the_shell_and_restore_the_terminal() -> Result<()> {
        if run_in_test_terminal(test_name!(
            termination_signals_stop_the_shell_and_restore_the_terminal
        ))? {
            return Ok(());
        }
        let before = terminal_settings()?;

        for signal in [SIGHUP, SIGINT, SIGQUIT, SIGTERM] {
            let session = ShellSession::start(shell_command("read value"))?;
            let shell_pid = session
                .shell
                .child
                .process_id()
                .context("shell should have a process ID")?;
            assert!(raw_mode_enabled()?);

            // Queue the signal after registration, before the event loop starts.
            signal_hook::low_level::raise(signal)?;
            let error = session
                .run()
                .err()
                .context("the signal should stop the session")?;

            assert_eq!(
                error.downcast_ref::<TerminationSignal>().map(|stop| stop.0),
                Some(signal)
            );
            assert!(!raw_mode_enabled()?);
            assert_eq!(terminal_settings()?, before);
            let alive = Command::new("/bin/kill")
                .args(["-0", &shell_pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            assert!(!alive.success(), "the shell must be stopped and collected");
        }
        Ok(())
    }

    #[test]
    fn termination_closes_the_queue_before_worker_cleanup() -> Result<()> {
        let (sender, receiver) = sync_channel(2);
        sender.send(SessionEvent::Terminated(SIGTERM))?;
        sender.send(SessionEvent::Output(TOO_LATE.to_vec()))?;
        let mut output = Vec::new();

        let error = forward_session_events(receiver, &mut output, |_| Ok(()))
            .err()
            .context("termination should stop the loop")?;

        assert_eq!(
            error.to_string(),
            format!("session stopped by signal {SIGTERM}")
        );
        assert_eq!(
            error.downcast_ref::<TerminationSignal>().map(|stop| stop.0),
            Some(SIGTERM)
        );
        assert!(output.is_empty());
        assert!(queue_is_closed(&sender));
        Ok(())
    }

    #[test]
    fn session_panic_restores_terminal_before_returning_an_error() -> Result<()> {
        if run_in_test_terminal(test_name!(
            session_panic_restores_terminal_before_returning_an_error
        ))? {
            return Ok(());
        }
        let before = terminal_settings()?;
        let command = shell_command("read value");

        let result = catch_panic::<()>("session panicked", || {
            let _session = ShellSession::start(command)?;
            assert!(raw_mode_enabled()?);
            panic!("test session panic");
        });

        assert!(matches!(result, Err(error) if error.to_string() == "session panicked"));
        assert!(!raw_mode_enabled()?);
        assert_eq!(terminal_settings()?, before);
        Ok(())
    }

    #[test]
    fn resize_panic_closes_the_session_before_cleanup() -> Result<()> {
        let (sender, receiver) = sync_channel(2);
        sender.send(SessionEvent::Resized)?;
        sender.send(SessionEvent::Output(TOO_LATE.to_vec()))?;
        let mut output = Vec::new();

        let result = forward_session_events(receiver, &mut output, |_| {
            panic!("test resize panic");
        });

        assert!(matches!(result, Err(error) if error.to_string() == "resize handler panicked"));
        assert!(output.is_empty());
        assert!(queue_is_closed(&sender));
        Ok(())
    }

    #[test]
    fn session_resizes_between_flushed_output_events() -> Result<()> {
        /// Records writes, and a `|` for every flush.
        struct Recorder<'a>(&'a RefCell<Vec<u8>>);
        impl Write for Recorder<'_> {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.borrow_mut().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.0.borrow_mut().push(b'|');
                Ok(())
            }
        }
        let (sender, receiver) = sync_channel(4);
        sender.send(SessionEvent::Output(b"before".to_vec()))?;
        sender.send(SessionEvent::Resized)?;
        sender.send(SessionEvent::Output(b"after".to_vec()))?;
        sender.send(SessionEvent::OutputFinished(Ok(())))?;
        let calls = RefCell::new(Vec::new());
        let session_thread = thread::current().id();

        forward_session_events(receiver, Recorder(&calls), |_| {
            assert_eq!(thread::current().id(), session_thread);
            calls.borrow_mut().extend_from_slice(b"resize|");
            Ok(())
        })?;

        assert_eq!(*calls.borrow(), b"before|resize|after|");
        Ok(())
    }

    #[test]
    fn session_events_flush_output_after_input_finishes() -> Result<()> {
        let (sender, receiver) = sync_channel(4);
        sender.send(SessionEvent::Output(b"first\n".to_vec()))?;
        sender.send(SessionEvent::InputFinished(Ok(())))?;
        sender.send(SessionEvent::Output(b"\x1b[32mshell> ".to_vec()))?;
        sender.send(SessionEvent::OutputFinished(Ok(())))?;
        let mut bytes = Vec::new();
        let mut output = BufWriter::new(&mut bytes);

        forward_session_events(receiver, &mut output, |_| Ok(()))?;

        assert!(
            output.buffer().is_empty(),
            "the final prompt must be flushed"
        );
        drop(output);
        assert_eq!(bytes, b"first\n\x1b[32mshell> ");
        assert!(queue_is_closed(&sender));
        Ok(())
    }

    #[test]
    fn session_events_return_worker_errors_and_close_the_receiver() -> Result<()> {
        let failure = || io::Error::new(ErrorKind::PermissionDenied, "test failure").into();
        let failing_events = [
            SessionEvent::InputFinished(Err(failure())),
            SessionEvent::OutputFinished(Err(failure())),
            SessionEvent::ResizeFailed(failure()),
            // The resize handler below fails too.
            SessionEvent::Resized,
        ];
        for event in failing_events {
            let (sender, receiver) = sync_channel(3);
            sender.send(event)?;
            sender.send(SessionEvent::Output(TOO_LATE.to_vec()))?;
            sender.send(SessionEvent::OutputFinished(Ok(())))?;
            let mut output = Vec::new();

            let error = forward_session_events(receiver, &mut output, |_| Err(failure()))
                .err()
                .context("the worker error should stop the loop")?;

            assert_eq!(
                error.downcast_ref::<io::Error>().map(io::Error::kind),
                Some(ErrorKind::PermissionDenied)
            );
            assert!(
                output.is_empty(),
                "output after the failure must not be written"
            );
            assert!(queue_is_closed(&sender));
        }
        Ok(())
    }

    #[test]
    fn session_events_require_output_completion_before_disconnect() -> Result<()> {
        let (sender, receiver) = sync_channel(1);
        sender.send(SessionEvent::InputFinished(Ok(())))?;
        drop(sender);

        let result = forward_session_events(receiver, io::sink(), |_| Ok(()));

        assert!(matches!(
            result,
            Err(error) if error.to_string() == "session event channel closed unexpectedly"
        ));
        Ok(())
    }

    #[test]
    fn session_events_report_terminal_write_and_flush_failures() -> Result<()> {
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

        for (fail_on_flush, expected) in [
            (false, "could not write shell output"),
            (true, "could not flush shell output"),
        ] {
            let (sender, receiver) = sync_channel(2);
            sender.send(SessionEvent::Output(b"shell> ".to_vec()))?;
            sender.send(SessionEvent::OutputFinished(Ok(())))?;

            let error =
                forward_session_events(receiver, FailingWriter { fail_on_flush }, |_| Ok(()))
                    .err()
                    .context("the terminal failure should stop the loop")?;

            assert_eq!(error.to_string(), expected);
            assert_eq!(
                error.downcast_ref::<io::Error>().map(io::Error::kind),
                Some(ErrorKind::BrokenPipe)
            );
            assert!(queue_is_closed(&sender));
        }
        Ok(())
    }

    #[test]
    fn session_start_restores_terminal_after_spawn_failure() -> Result<()> {
        if run_in_test_terminal(test_name!(
            session_start_restores_terminal_after_spawn_failure
        ))? {
            return Ok(());
        }
        let before = terminal_settings()?;
        assert!(!raw_mode_enabled()?);

        let result = ShellSession::start(CommandBuilder::new("/dev/null/stray-shell"));

        assert!(matches!(result, Err(error) if error.to_string() == "could not start shell"));
        assert!(!raw_mode_enabled()?);
        assert_eq!(terminal_settings()?, before);
        Ok(())
    }

    #[test]
    fn session_drop_restores_terminal_after_success() -> Result<()> {
        if run_in_test_terminal(test_name!(session_drop_restores_terminal_after_success))? {
            return Ok(());
        }
        let before = terminal_settings()?;

        let mut session = ShellSession::start(shell_command("exit 7"))?;

        assert!(raw_mode_enabled()?);
        assert_ne!(terminal_settings()?, before);
        let size = session.master.get_size()?;
        let expected = PtySize::default();
        assert_eq!((size.rows, size.cols), (expected.rows, expected.cols));
        let status = session.shell.child.wait()?;
        assert_eq!(status.exit_code(), 7);

        drop(session);

        assert!(!raw_mode_enabled()?);
        assert_eq!(terminal_settings()?, before);
        Ok(())
    }
}
