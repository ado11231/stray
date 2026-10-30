//! The startup script that `stray init` prints for bash and zsh.

use crate::Shell;
use stray_cat::escape::PROMPT_MARK;

/// Replaced with the quoted path to the stray program.
const EXECUTABLE_PLACEHOLDER: &str = "@STRAY_EXECUTABLE@";

/// Replaced with the OSC number of the prompt mark.
const PROMPT_MARK_PLACEHOLDER: &str = "@STRAY_PROMPT_MARK@";

/// Returns the startup script for `shell`, set up to run `executable`.
pub fn init_script(shell: Shell, executable: &str) -> String {
    let script = match shell {
        Shell::Bash => include_str!("shell/bash.sh"),
        Shell::Zsh => include_str!("shell/zsh.sh"),
    };
    script
        .replace(EXECUTABLE_PLACEHOLDER, &quote(executable))
        .replace(PROMPT_MARK_PLACEHOLDER, PROMPT_MARK)
}

/// Wraps a value in single quotes, so the shell reads it exactly as written.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrapper::{SHELL_MARKER, ShellGuard, forward_output};
    use anyhow::{Context, Result};
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::{Read, Write};
    use std::process::Command;
    use std::time::Duration;
    use stray_cat::env;

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

    /// The prompt the test shells show, so the test knows when to type.
    const TEST_PROMPT: &str = "READY> ";

    /// How long a test shell may take to show its prompt or to exit.
    const SHELL_WAIT: Duration = Duration::from_secs(10);

    /// Reads shell output into `bytes` until it holds `count` prompts.
    fn wait_for_prompts(
        receiver: &std::sync::mpsc::Receiver<Vec<u8>>,
        bytes: &mut Vec<u8>,
        count: usize,
    ) -> Result<()> {
        let prompt = TEST_PROMPT.as_bytes();
        let deadline = std::time::Instant::now() + SHELL_WAIT;
        while bytes
            .windows(prompt.len())
            .filter(|part| *part == prompt)
            .count()
            < count
        {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let chunk = receiver
                .recv_timeout(left)
                .context("the shell did not show its prompt")?;
            bytes.extend(chunk);
        }
        Ok(())
    }

    /// Starts `shell` as Stray's own inner shell with the startup script in its settings
    /// file, types each line of `lines`, and returns the exit status in every prompt mark.
    fn prompt_marks(shell: Shell, lines: &[&str]) -> Result<Vec<String>> {
        let folder = std::env::temp_dir().join(format!(
            "stray-prompt-mark-{}-{}",
            std::process::id(),
            shell_path(shell).replace('/', "-")
        ));
        std::fs::create_dir_all(&folder)?;
        let settings = folder.join(match shell {
            Shell::Bash => "bashrc",
            Shell::Zsh => ".zshrc",
        });
        // A prompt the test can spot, so it types only once the shell is ready.
        let script = init_script(shell, FAKE_EXECUTABLE);
        std::fs::write(&settings, format!("{script}\nPS1='{TEST_PROMPT}'\n"))?;

        let pty = native_pty_system().openpty(PtySize::default())?;
        let mut reader = pty.master.try_clone_reader()?;
        let mut writer = pty.master.take_writer()?;
        let mut command = CommandBuilder::new(shell_path(shell));
        if matches!(shell, Shell::Bash) {
            command.args(["--noprofile", "--rcfile"]);
            command.arg(&settings);
        }
        command.arg("-i");
        command.env("ZDOTDIR", &folder);
        command.env("HISTFILE", folder.join("history"));
        command.env("PATH", "/bin:/usr/bin");
        let tty_name = pty
            .master
            .tty_name()
            .context("test terminal should have a name")?;
        command.env(SHELL_MARKER, tty_name);
        let mut child = ShellGuard {
            child: pty.slave.spawn_command(command)?,
        };
        drop(pty.slave);

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            // Stops when the shell exits and its terminal closes.
            while let Ok(count @ 1..) = reader.read(&mut buffer) {
                if sender.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        // Each line waits for its prompt, as a person typing would. Some systems take a
        // while to start zsh, and keys typed too early can be lost.
        let mut bytes = Vec::new();
        for (typed, line) in lines.iter().chain(&["exit"]).enumerate() {
            wait_for_prompts(&receiver, &mut bytes, typed + 1)?;
            writer.write_all(format!("{line}\r").as_bytes())?;
        }
        // Collect the rest until the shell exits and the reader stops.
        while let Ok(chunk) = receiver.recv_timeout(SHELL_WAIT) {
            bytes.extend(chunk);
        }
        child.child.wait()?;
        std::fs::remove_dir_all(&folder)?;

        let text = String::from_utf8_lossy(&bytes);
        let start = format!("\x1b]{PROMPT_MARK};");
        Ok(text
            .split(start.as_str())
            .skip(1)
            .filter_map(|after| after.split_once('\x07'))
            .map(|(status, _)| status.to_owned())
            .collect())
    }

    #[test]
    fn the_inner_shell_marks_each_prompt_after_a_command() -> Result<()> {
        for shell in SHELLS {
            let marks = prompt_marks(shell, &["false", "", "true"])?;
            // The empty line ran nothing, so it has no mark.
            assert_eq!(marks, ["1", "0"], "{}", shell_path(shell));
        }
        Ok(())
    }

    #[test]
    fn an_outer_shell_has_no_prompt_mark() {
        for shell in SHELLS {
            let script = init_script(shell, FAKE_EXECUTABLE);
            // The marks live only inside the block for Stray's own shell.
            let (outer, inner) = script
                .split_once("Inside Stray's own shell")
                .unwrap_or((&script, ""));
            assert!(!outer.contains(PROMPT_MARK), "{}", shell_path(shell));
            assert!(inner.contains(PROMPT_MARK), "{}", shell_path(shell));
        }
    }
}
