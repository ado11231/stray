mod image_check;
mod install;
mod shell;
mod terminal;
mod wrapper;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "stray", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Install,
    Uninstall,
    // Start and Init are helpers for shell startup, so they stay out of normal help.
    #[command(hide = true)]
    Start {
        #[arg(long)]
        login: bool,
        #[arg(long)]
        shell: Option<std::ffi::OsString>,
    },
    #[command(hide = true)]
    Init {
        shell: Shell,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Shell {
    Zsh,
    Bash,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    // Report failures after session cleanup, while the terminal is usable again.
    std::panic::set_hook(Box::new(|_| {}));
    match wrapper::catch_panic("session panicked", || run(cli)) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("stray: {}", error.to_string().replace(['\r', '\n'], " "));
            match error.downcast_ref::<wrapper::TerminationSignal>() {
                Some(signal) => std::process::ExitCode::from((128 + signal.0) as u8),
                None => std::process::ExitCode::FAILURE,
            }
        }
    }
}

fn run(cli: Cli) -> Result<std::process::ExitCode> {
    match cli.command {
        Command::Install | Command::Uninstall => {
            install::run(matches!(cli.command, Command::Install))?;
            Ok(std::process::ExitCode::SUCCESS)
        }
        Command::Start { login, shell } => {
            let shell_path = shell
                .or_else(|| std::env::var_os("SHELL"))
                .context("SHELL is not set")?;
            let command = wrapper::build_shell_command(&shell_path, login);
            let check = match image_check::probe::run(std::env::var_os("TMUX").is_some()) {
                Ok(check) => check,
                Err(error) => {
                    if error.downcast_ref::<wrapper::TerminationSignal>().is_some() {
                        return Err(error);
                    }
                    eprintln!("stray: {error}; starting shell directly");
                    return wrapper::run_shell_directly(&shell_path, login);
                }
            };
            if !check.supported {
                image_check::hint::show_once();
                if check.input.is_empty() {
                    return wrapper::run_shell_directly(&shell_path, login);
                }
            }

            match wrapper::ShellSession::start(command) {
                Ok(session) => session.run_with_input(check.input),
                Err(error) => {
                    eprintln!("stray: {error}; starting shell directly");
                    wrapper::run_shell_directly(&shell_path, login)
                }
            }
        }
        Command::Init { shell } => {
            let executable = std::env::current_exe().context("could not locate stray")?;
            let executable = executable
                .to_str()
                .context("stray path is not valid UTF-8")?;
            use std::io::Write;
            std::io::stdout()
                .lock()
                .write_all(shell::init_script(shell, executable).as_bytes())?;
            Ok(std::process::ExitCode::SUCCESS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn stats_is_no_longer_a_command() {
        let error = Cli::try_parse_from(["stray", "stats"]).err().unwrap();

        assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn remaining_commands_are_recognized() {
        let install = Cli::try_parse_from(["stray", "install"]).unwrap();
        let uninstall = Cli::try_parse_from(["stray", "uninstall"]).unwrap();
        let start = Cli::try_parse_from(["stray", "start"]).unwrap();

        assert!(matches!(install.command, Command::Install));
        assert!(matches!(uninstall.command, Command::Uninstall));
        assert!(matches!(start.command, Command::Start { login: false, .. }));
    }

    #[test]
    fn start_accepts_an_explicit_shell_path() {
        let cli =
            Cli::try_parse_from(["stray", "start", "--shell", "/tmp/my shell", "--login"]).unwrap();
        match cli.command {
            Command::Start {
                shell: Some(path),
                login: true,
            } => assert_eq!(path, "/tmp/my shell"),
            _ => panic!("expected a login shell with an explicit path"),
        }
    }

    #[test]
    fn start_accepts_login_flag() {
        let cli = Cli::try_parse_from(["stray", "start", "--login"]).unwrap();

        assert!(matches!(cli.command, Command::Start { login: true, .. }));
    }

    #[test]
    fn init_accepts_supported_shells() {
        let zsh = Cli::try_parse_from(["stray", "init", "zsh"]).unwrap();
        let bash = Cli::try_parse_from(["stray", "init", "bash"]).unwrap();

        assert!(matches!(zsh.command, Command::Init { shell: Shell::Zsh }));
        assert!(matches!(bash.command, Command::Init { shell: Shell::Bash }));
    }

    #[test]
    fn init_rejects_missing_or_unsupported_shells() {
        let missing = Cli::try_parse_from(["stray", "init"]).err().unwrap();
        let unsupported = Cli::try_parse_from(["stray", "init", "fish"])
            .err()
            .unwrap();

        assert_eq!(missing.kind(), ErrorKind::MissingRequiredArgument);
        assert_eq!(unsupported.kind(), ErrorKind::InvalidValue);
    }
}
