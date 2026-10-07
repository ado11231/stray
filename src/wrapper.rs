use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, PtyPair, PtySize, native_pty_system};
use std::ffi::OsStr;

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

/// Opens terminal pair that Stray and shell will use.
pub fn open_pty(size: PtySize) -> Result<PtyPair> {
    native_pty_system()
        .openpty(size)
        .context("could not open PTY")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

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
