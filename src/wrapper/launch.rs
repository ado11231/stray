//! Opens the PTY and starts the shell, or starts the shell directly without Stray.

use super::SHELL_MARKER;
use crate::terminal;
use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtyPair, PtySize, native_pty_system};
use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

/// Interactive mode lets the shell show a prompt and accept typed commands.
const INTERACTIVE_FLAG: &str = "-i";

/// Login mode loads startup files such as zsh's .zprofile.
const LOGIN_FLAG: &str = "-l";

/// Stops and collects the shell when it leaves scope.
pub struct ShellGuard {
    pub child: Box<dyn Child + Send + Sync>,
}

impl Drop for ShellGuard {
    fn drop(&mut self) {
        // A shell that already finished does not need further cleanup.
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        // Stop the shell, then collect its exit status so it does not linger.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Prepares the shell command for the PTY without starting a process.
pub fn build_shell_command(shell_path: &OsStr, login: bool) -> CommandBuilder {
    let mut command = CommandBuilder::new(shell_path);
    command.arg(INTERACTIVE_FLAG);
    if login {
        command.arg(LOGIN_FLAG);
    }
    command
}

/// Replaces Stray with the shell on the existing terminal. Returns only on failure.
pub fn run_shell_directly(shell_path: &OsStr, login: bool) -> Result<ExitCode> {
    let error = build_direct_shell_command(shell_path, login).exec();
    Err(error).context("could not start shell directly")
}

fn build_direct_shell_command(shell_path: &OsStr, login: bool) -> Command {
    let mut command = Command::new(shell_path);
    command.arg(INTERACTIVE_FLAG);
    if login {
        command.arg(LOGIN_FLAG);
    }
    // Mark this terminal, so the startup script does not start Stray again here.
    match terminal::tty_name() {
        Some(tty_name) => command.env(SHELL_MARKER, tty_name),
        None => command.env_remove(SHELL_MARKER),
    };
    command
}

/// Opens the terminal pair that Stray and the shell will use.
pub fn open_pty(size: PtySize) -> Result<PtyPair> {
    native_pty_system()
        .openpty(size)
        .context("could not open PTY")
}

/// Updates the shell's terminal size and notifies its foreground program.
pub fn resize_pty(master: &dyn MasterPty, size: PtySize) -> Result<()> {
    master.resize(size).context("could not resize PTY")
}

/// Starts the shell on the shell's side of the PTY.
pub fn spawn_shell(pty: &PtyPair, command: CommandBuilder) -> Result<Box<dyn Child + Send + Sync>> {
    pty.slave
        .spawn_command(command)
        .context("could not start shell")
}

#[cfg(test)]
mod tests {
    use super::super::test_terminal::{run_in_test_terminal, test_name, without_crate_name};
    use super::*;
    use anyhow::ensure;
    use std::ffi::OsString;
    use std::io::{self, ErrorKind};
    use std::sync::{Arc, Mutex};

    /// A path that can never be run, because /dev/null is a file and not a folder.
    const MISSING_SHELL: &str = "/dev/null/stray-shell";

    #[test]
    fn direct_shell_marker_matches_the_existing_terminal() -> Result<()> {
        if run_in_test_terminal(test_name!(
            direct_shell_marker_matches_the_existing_terminal
        ))? {
            return Ok(());
        }
        let terminal_name = terminal::tty_name().context("test should have a terminal")?;
        let mut command = build_direct_shell_command(OsStr::new("/bin/sh"), false);
        command.args(["-c", "printf '%s\\n' \"$STRAY_TTY\"; /usr/bin/tty"]);
        command.env("ENV", "/dev/null");
        command.stdin(std::process::Stdio::inherit());

        let output = command.output()?;

        assert!(output.status.success());
        let text = String::from_utf8(output.stdout)?;
        let lines: Vec<_> = text.lines().map(OsStr::new).collect();
        assert_eq!(lines, vec![terminal_name.as_os_str(); 2]);
        Ok(())
    }

    #[test]
    fn direct_shell_failure_preserves_the_os_error() -> Result<()> {
        // Even a failed exec changes signal handling, so isolate it from other tests.
        const CHILD_TEST: &str = "STRAY_TEST_DIRECT_SHELL_FAILURE";
        if std::env::var_os(CHILD_TEST).is_none() {
            let test_path = test_name!(direct_shell_failure_preserves_the_os_error);
            let output = Command::new(std::env::current_exe()?)
                .args(["--exact", without_crate_name(test_path), "--nocapture"])
                .env(CHILD_TEST, "1")
                .output()
                .context("isolated test should start")?;

            ensure!(
                output.status.success(),
                "isolated test failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(());
        }

        for login in [false, true] {
            let error = run_shell_directly(OsStr::new(MISSING_SHELL), login)
                .err()
                .context("invalid shell path should fail")?;

            assert_eq!(error.to_string(), "could not start shell directly");
            assert_eq!(
                error.downcast_ref::<io::Error>().map(io::Error::kind),
                Some(ErrorKind::NotADirectory)
            );
        }
        Ok(())
    }

    /// A pretend shell that records which cleanup calls it received.
    #[derive(Clone, Debug)]
    struct TestChild {
        finished: bool,
        poll_fails: bool,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl TestChild {
        fn record(&self, call: &'static str) {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(call);
            }
        }
    }

    impl portable_pty::ChildKiller for TestChild {
        fn kill(&mut self) -> io::Result<()> {
            self.record("kill");
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            Box::new(self.clone())
        }
    }

    impl Child for TestChild {
        fn try_wait(&mut self) -> io::Result<Option<portable_pty::ExitStatus>> {
            self.record("try_wait");
            if self.poll_fails {
                return Err(io::Error::other("could not check child"));
            }
            Ok(self
                .finished
                .then(|| portable_pty::ExitStatus::with_exit_code(7)))
        }

        fn wait(&mut self) -> io::Result<portable_pty::ExitStatus> {
            self.record("wait");
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

    /// Drops a guard around a pretend shell and returns the calls it made.
    fn cleanup_calls(finished: bool, poll_fails: bool) -> Vec<&'static str> {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let guard = ShellGuard {
            child: Box::new(TestChild {
                finished,
                poll_fails,
                calls: Arc::clone(&calls),
            }),
        };
        drop(guard);
        calls.lock().map(|calls| calls.to_vec()).unwrap_or_default()
    }

    #[test]
    fn shell_guard_stops_and_collects_a_running_shell() {
        assert_eq!(cleanup_calls(false, false), ["try_wait", "kill", "wait"]);
    }

    #[test]
    fn shell_guard_leaves_a_finished_shell_alone() {
        assert_eq!(cleanup_calls(true, false), ["try_wait"]);
    }

    #[test]
    fn shell_guard_attempts_cleanup_when_checking_the_shell_fails() {
        assert_eq!(cleanup_calls(false, true), ["try_wait", "kill", "wait"]);
    }

    #[test]
    fn shell_starts_and_returns_its_exit_code() -> Result<()> {
        let pty = open_pty(PtySize::default())?;
        let mut command = CommandBuilder::new("/bin/sh");
        // Exit immediately so the test never waits for keyboard input.
        command.args(["-c", "exit 7"]);

        let mut child = spawn_shell(&pty, command)?;
        drop(pty.slave);
        let status = child.wait()?;

        assert_eq!(status.exit_code(), 7);
        Ok(())
    }

    #[test]
    fn missing_shell_returns_an_error() -> Result<()> {
        let pty = open_pty(PtySize::default())?;

        let error = spawn_shell(&pty, CommandBuilder::new(MISSING_SHELL))
            .err()
            .context("missing shell should fail")?;

        assert_eq!(error.to_string(), "could not start shell");
        Ok(())
    }

    fn size(rows: u16, cols: u16) -> PtySize {
        PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    #[test]
    fn pty_uses_requested_size() -> Result<()> {
        for (rows, cols) in [(24, 80), (40, 120), (10, 30)] {
            // Read the size from a real PTY to check what the OS received.
            let pty = open_pty(size(rows, cols))?;

            assert_eq!(pty.master.get_size()?, size(rows, cols));
        }
        Ok(())
    }

    #[test]
    fn pty_resizes_larger_and_smaller() -> Result<()> {
        let pty = open_pty(PtySize::default())?;

        for (rows, cols) in [(40, 120), (10, 30), (24, 80)] {
            resize_pty(pty.master.as_ref(), size(rows, cols))?;

            assert_eq!(pty.master.get_size()?, size(rows, cols));
        }
        Ok(())
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
