//! Runs the shell inside a PTY that Stray controls, and puts the terminal back on every exit.
//!
//! * `launch`: opens the PTY and starts the shell, or starts the shell directly.
//! * `session`: the main loop that passes shell output, keys, and resizes along.
//! * `workers`: the threads that read keys, read shell output, and watch signals.

mod launch;
mod session;
#[cfg(test)]
mod test_terminal;
mod workers;

pub use launch::{build_shell_command, run_shell_directly};
pub use session::ShellSession;

#[cfg(test)]
pub use launch::ShellGuard;
#[cfg(test)]
pub use workers::forward_output;

/// Holds the name of the terminal the shell runs in. The startup script skips Stray when
/// this matches the current terminal, so Stray never starts inside itself.
pub const SHELL_MARKER: &str = "STRAY_TTY";

/// The session was stopped by a signal such as SIGTERM.
#[derive(Debug)]
pub struct TerminationSignal(pub i32);

impl std::fmt::Display for TerminationSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "session stopped by signal {}", self.0)
    }
}

impl std::error::Error for TerminationSignal {}

/// Converts an unwinding panic into an error after the closure's guards are dropped.
pub fn catch_panic<T>(
    message: &'static str,
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .map_err(|_| anyhow::anyhow!(message))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, ErrorKind};

    #[test]
    fn panic_boundary_preserves_success_and_regular_errors() -> anyhow::Result<()> {
        assert_eq!(catch_panic("unexpected panic", || Ok(7))?, 7);

        let error = catch_panic::<()>("unexpected panic", || {
            Err(io::Error::new(ErrorKind::BrokenPipe, "test failure").into())
        })
        .err()
        .ok_or_else(|| anyhow::anyhow!("the error should pass through"))?;

        assert_eq!(
            error.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(ErrorKind::BrokenPipe)
        );
        Ok(())
    }

    #[test]
    fn panic_boundary_turns_a_panic_into_an_error() {
        let result = catch_panic::<()>("unexpected panic", || panic!("test panic"));

        assert!(matches!(result, Err(error) if error.to_string() == "unexpected panic"));
    }
}
