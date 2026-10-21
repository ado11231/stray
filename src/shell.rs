//! The startup script that `stray init` prints for bash and zsh.

use crate::Shell;

/// Replaced with the quoted path to the stray program.
const EXECUTABLE_PLACEHOLDER: &str = "@STRAY_EXECUTABLE@";

/// Returns the startup script for `shell`, set up to run `executable`.
pub fn init_script(shell: Shell, executable: &str) -> String {
    let script = match shell {
        Shell::Bash => include_str!("shell/bash.sh"),
        Shell::Zsh => include_str!("shell/zsh.sh"),
    };
    script.replace(EXECUTABLE_PLACEHOLDER, &quote(executable))
}

/// Wraps a value in single quotes, so the shell reads it exactly as written.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env;
    use crate::wrapper::{SHELL_MARKER, ShellGuard, forward_output};
    use anyhow::{Context, Result};
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::process::Command;
    use std::time::Duration;

    const SHELLS: [Shell; 2] = [Shell::Bash, Shell::Zsh];

    /// Printed when the startup script decides not to start Stray.
    const SKIPPED: &str = "SKIPPED";

    /// Stands in for stray, so the test sees the arguments it would get.
    const FAKE_EXECUTABLE: &str = "/bin/echo";

    /// How the shell's own terminal is passed as the marker in `evaluate`.
    const OWN_TERMINAL: &str = "own";

    fn shell_path(shell: Shell) -> &'static str {
        match shell {
            Shell::Bash => "/bin/bash",
            Shell::Zsh => "/bin/zsh",
        }
    }

    /// Flags that make the shell skip the user's own settings files.
    fn no_settings_flags(shell: Shell) -> &'static [&'static str] {
        match shell {
            Shell::Bash => &["--noprofile", "--norc"],
            Shell::Zsh => &["-f"],
        }
    }

    /// The startup script, followed by a line that only runs if Stray was not started.
    fn script_then_skipped(shell: Shell) -> String {
        format!("{}\nprintf {SKIPPED}", init_script(shell, FAKE_EXECUTABLE))
    }

    /// Settings for one run of the startup script.
    struct Case<'a> {
        login: bool,
        marker: &'a str,
        ssh: bool,
        tmux: bool,
    }

    /// Runs the startup script in a real terminal and returns what it printed.
    fn evaluate(shell: Shell, case: Case) -> Result<String> {
        let pty = native_pty_system().openpty(PtySize::default())?;
        let reader = pty.master.try_clone_reader()?;
        let mut command = CommandBuilder::new(shell_path(shell));
        command.args(no_settings_flags(shell));
        command.arg("-i");
        if case.login {
            command.arg("-l");
        }
        command.args(["-c", &script_then_skipped(shell)]);

        let tty_name = pty
            .master
            .tty_name()
            .context("test terminal should have a name")?;
        let marker = if case.marker == OWN_TERMINAL {
            tty_name.to_str().context("terminal name should be text")?
        } else {
            case.marker
        };
        command.env(SHELL_MARKER, marker);
        command.env("PATH", "/bin:/usr/bin");
        command.env(env::SHELL, "/deliberately/wrong");
        command.env(
            "SSH_CONNECTION",
            if case.ssh { "test connection" } else { "" },
        );
        command.env(env::TMUX, if case.tmux { "test tmux" } else { "" });
        let mut child = ShellGuard {
            child: pty.slave.spawn_command(command)?,
        };
        drop(pty.slave);

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = forward_output(reader, &mut bytes).map(|()| bytes);
            let _ = sender.send(result);
        });
        let bytes = receiver
            .recv_timeout(Duration::from_secs(5))
            .context("shell init timed out")??;
        anyhow::ensure!(child.child.wait()?.success(), "shell should succeed");
        Ok(String::from_utf8(bytes)?.trim().to_owned())
    }

    #[test]
    fn init_preserves_login_mode_and_starts_new_windows_and_standalone_tmux() -> Result<()> {
        for shell in SHELLS {
            for login in [false, true] {
                for (marker, tmux) in [("", false), ("/dev/stale", false), ("", true)] {
                    let case = Case {
                        login,
                        marker,
                        ssh: false,
                        tmux,
                    };
                    let output = evaluate(shell, case)?;
                    let arguments = output
                        .strip_prefix("start --shell ")
                        .context("stray should be started")?;
                    let actual_path = if login {
                        arguments
                            .strip_suffix(" --login")
                            .context("login mode should be kept")?
                    } else {
                        arguments
                    };
                    // Linux may resolve /bin/zsh through /usr/bin after startup.
                    assert_eq!(
                        std::fs::canonicalize(actual_path)?,
                        std::fs::canonicalize(shell_path(shell))?
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn init_skips_own_shell_ssh_and_tmux_with_an_inherited_marker() -> Result<()> {
        for shell in SHELLS {
            for (marker, ssh, tmux) in [
                (OWN_TERMINAL, false, false),
                ("", true, false),
                ("/dev/stale", false, true),
            ] {
                let case = Case {
                    login: false,
                    marker,
                    ssh,
                    tmux,
                };
                assert_eq!(evaluate(shell, case)?, SKIPPED);
            }
        }
        Ok(())
    }

    #[test]
    fn init_skips_noninteractive_or_redirected_shells() -> Result<()> {
        for shell in SHELLS {
            for interactive in [false, true] {
                let mut command = Command::new(shell_path(shell));
                command.args(no_settings_flags(shell));
                if interactive {
                    command.arg("-i");
                }
                // Output goes to a pipe here, not a terminal, so Stray should step aside.
                let output = command
                    .arg("-c")
                    .arg(script_then_skipped(shell))
                    .env("ENV", "/dev/null")
                    .output()?;
                assert!(output.status.success());
                assert_eq!(output.stdout, SKIPPED.as_bytes());
            }
        }
        Ok(())
    }

    #[test]
    fn executable_paths_are_quoted_literally() -> Result<()> {
        let path = "/tmp/stray 'quoted' $(printf wrong)";
        for shell in SHELLS {
            let output = Command::new(shell_path(shell))
                .args(["-f", "-c", &format!("printf '%s' {}", quote(path))])
                .output()?;
            assert!(output.status.success());
            assert_eq!(output.stdout, path.as_bytes());
        }
        Ok(())
    }
}
