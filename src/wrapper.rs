use std::ffi::OsStr;

use portable_pty::CommandBuilder;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

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
