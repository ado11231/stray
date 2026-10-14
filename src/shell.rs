use crate::Shell;

pub fn init_script(shell: Shell, executable: &str) -> String {
    let (shell_path, login_check) = match shell {
        Shell::Bash => ("\"$BASH\"", "shopt -q login_shell"),
        Shell::Zsh => ("\"${commands[zsh]:-/bin/zsh}\"", "[[ -o login ]]"),
    };
    let executable = quote(executable);
    format!(
        r#"if [[ $- == *i* && -t 0 && -t 1 && -z ${{SSH_CONNECTION-}} ]] &&
   [[ -z ${{TMUX-}} || -z ${{STRAY_TTY-}} ]] &&
   [[ ${{STRAY_TTY-}} != "$(/usr/bin/tty 2>/dev/null)" ]]; then
    if {login_check}; then
        exec {executable} start --shell {shell_path} --login
    else
        exec {executable} start --shell {shell_path}
    fi
fi
"#
    )
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrapper::{ShellGuard, forward_output};
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::process::Command;
    use std::time::Duration;

    fn evaluate(shell: Shell, login: bool, marker: &str, ssh: bool, tmux: bool) -> String {
        let pty = native_pty_system().openpty(PtySize::default()).unwrap();
        let reader = pty.master.try_clone_reader().unwrap();
        let path = match shell {
            Shell::Bash => "/bin/bash",
            Shell::Zsh => "/bin/zsh",
        };
        let mut command = CommandBuilder::new(path);
        match shell {
            Shell::Bash => command.args(["--noprofile", "--norc"]),
            Shell::Zsh => command.arg("-f"),
        }
        command.arg("-i");
        if login {
            command.arg("-l");
        }
        command.args([
            "-c",
            &format!("{}\nprintf SKIPPED", init_script(shell, "/bin/echo")),
        ]);
        let tty_name = pty.master.tty_name().unwrap();
        let marker = if marker == "own" {
            tty_name.to_str().unwrap()
        } else {
            marker
        };
        command.env("STRAY_TTY", marker);
        command.env("PATH", "/bin:/usr/bin");
        command.env("SHELL", "/deliberately/wrong");
        command.env("SSH_CONNECTION", if ssh { "test connection" } else { "" });
        command.env("TMUX", if tmux { "test tmux" } else { "" });
        let mut child = ShellGuard {
            child: pty.slave.spawn_command(command).unwrap(),
        };
        drop(pty.slave);
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            forward_output(reader, &mut bytes).unwrap();
            let _ = sender.send(bytes);
        });
        let bytes = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("shell init timed out");
        assert!(child.child.wait().unwrap().success());
        String::from_utf8(bytes).unwrap().trim().to_owned()
    }

    #[test]
    fn init_preserves_login_mode_and_starts_new_windows_and_standalone_tmux() {
        for shell in [Shell::Bash, Shell::Zsh] {
            for login in [false, true] {
                for (marker, tmux) in [("", false), ("/dev/stale", false), ("", true)] {
                    let output = evaluate(shell, login, marker, false, tmux);
                    let shell_path = match shell {
                        Shell::Bash => "/bin/bash",
                        Shell::Zsh => "/bin/zsh",
                    };
                    let suffix = if login { " --login" } else { "" };
                    assert_eq!(output, format!("start --shell {shell_path}{suffix}"));
                }
            }
        }
    }

    #[test]
    fn init_skips_own_shell_ssh_and_tmux_with_an_inherited_marker() {
        for shell in [Shell::Bash, Shell::Zsh] {
            for (marker, ssh, tmux) in [
                ("own", false, false),
                ("", true, false),
                ("/dev/stale", false, true),
            ] {
                assert_eq!(evaluate(shell, false, marker, ssh, tmux), "SKIPPED");
            }
        }
    }

    #[test]
    fn init_skips_noninteractive_or_redirected_shells() {
        for (shell, path) in [(Shell::Bash, "/bin/bash"), (Shell::Zsh, "/bin/zsh")] {
            for interactive in [false, true] {
                let mut command = Command::new(path);
                match shell {
                    Shell::Bash => command.args(["--noprofile", "--norc"]),
                    Shell::Zsh => command.arg("-f"),
                };
                if interactive {
                    command.arg("-i");
                }
                let output = command
                    .arg("-c")
                    .arg(format!(
                        "{}\nprintf SKIPPED",
                        init_script(shell, "/bin/echo")
                    ))
                    .env("ENV", "/dev/null")
                    .output()
                    .unwrap();
                assert!(output.status.success());
                assert_eq!(output.stdout, b"SKIPPED");
            }
        }
    }

    #[test]
    fn executable_paths_are_quoted_literally() {
        let path = "/tmp/stray 'quoted' $(printf wrong)";
        for shell in ["/bin/bash", "/bin/zsh"] {
            let output = Command::new(shell)
                .args(["-f", "-c", &format!("printf '%s' {}", quote(path))])
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, path.as_bytes());
        }
    }
}
