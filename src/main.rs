//! The `stray` command: installs the startup line and wraps the shell in a PTY.

mod env;
mod image_check;
mod install;
mod shell;
mod terminal;
mod wrapper;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "stray", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Adds the startup line to your shell settings.
    Install,
    /// Removes the startup line from your shell settings.
    Uninstall,
    // Start and Init are helpers for shell startup, so they stay out of normal help.
    #[command(hide = true)]
    Start {
        #[arg(long)]
        login: bool,
        #[arg(long)]
        shell: Option<OsString>,
    },
    #[command(hide = true)]
    Init { shell: Shell },
}

/// The shells Stray can start from.
#[derive(Clone, Copy, ValueEnum)]
enum Shell {
    Zsh,
    Bash,
}

/// Shells report "killed by signal N" with exit code 128 + N.
const SIGNAL_EXIT_BASE: i32 = 128;

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Stay quiet on panic. The error is printed below, after the terminal is restored.
    std::panic::set_hook(Box::new(|_| {}));
    match wrapper::catch_panic("session panicked", || run(cli)) {
        Ok(code) => code,
        Err(error) => {
            // Keep the message on one line, so it cannot break the shell prompt.
            eprintln!("stray: {}", error.to_string().replace(['\r', '\n'], " "));
            match error.downcast_ref::<wrapper::TerminationSignal>() {
                Some(signal) => signal_exit_code(signal.0),
                None => ExitCode::FAILURE,
            }
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Install => install::run(true)?,
        Command::Uninstall => install::run(false)?,
        Command::Start { login, shell } => return start(shell, login),
        Command::Init { shell } => print_init_script(shell)?,
    }
    Ok(ExitCode::SUCCESS)
}

/// Checks image support, then runs the shell inside Stray, or directly if that fails.
fn start(shell: Option<OsString>, login: bool) -> Result<ExitCode> {
    let shell_path = match shell {
        Some(path) => path,
        None => env::shell()?,
    };
    let inside_tmux = std::env::var_os(env::TMUX).is_some();

    let check = match image_check::probe::run(inside_tmux) {
        Ok(check) => check,
        // A shutdown signal should stop Stray, not start a shell.
        Err(error) if error.is::<wrapper::TerminationSignal>() => return Err(error),
        Err(error) => return fall_back(&error, &shell_path, login),
    };
    if !check.supported {
        image_check::hint::show_once();
        // With no early keys to pass on, the shell does not need Stray at all.
        if check.input.is_empty() {
            return wrapper::run_shell_directly(&shell_path, login);
        }
    }

    let command = wrapper::build_shell_command(&shell_path, login);
    match wrapper::ShellSession::start_with_bar(command, check.supported, inside_tmux, check.cursor)
    {
        Ok(session) => session.run_with_input(check.input),
        Err(error) => fall_back(&error, &shell_path, login),
    }
}

/// Reports why Stray could not wrap the shell, then runs the shell directly.
fn fall_back(error: &anyhow::Error, shell_path: &OsStr, login: bool) -> Result<ExitCode> {
    eprintln!("stray: {error}; starting shell directly");
    wrapper::run_shell_directly(shell_path, login)
}

/// Prints the script that `eval "$(stray init zsh)"` runs at shell startup.
fn print_init_script(shell: Shell) -> Result<()> {
    let executable = std::env::current_exe().context("could not locate stray")?;
    let executable = executable
        .to_str()
        .context("stray path is not valid UTF-8")?;
    std::io::stdout()
        .lock()
        .write_all(shell::init_script(shell, executable).as_bytes())?;
    Ok(())
}

fn signal_exit_code(signal: i32) -> ExitCode {
    u8::try_from(SIGNAL_EXIT_BASE + signal).map_or(ExitCode::FAILURE, ExitCode::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn parse(arguments: &[&str]) -> Result<Command, clap::Error> {
        Cli::try_parse_from(arguments).map(|cli| cli.command)
    }

    fn error_kind(arguments: &[&str]) -> Option<ErrorKind> {
        parse(arguments).err().map(|error| error.kind())
    }

    #[test]
    fn stats_is_no_longer_a_command() {
        assert_eq!(
            error_kind(&["stray", "stats"]),
            Some(ErrorKind::InvalidSubcommand)
        );
    }

    #[test]
    fn remaining_commands_are_recognized() -> Result<()> {
        assert!(matches!(parse(&["stray", "install"])?, Command::Install));
        assert!(matches!(
            parse(&["stray", "uninstall"])?,
            Command::Uninstall
        ));
        assert!(matches!(
            parse(&["stray", "start"])?,
            Command::Start { login: false, .. }
        ));
        Ok(())
    }

    #[test]
    fn start_accepts_an_explicit_shell_path() -> Result<()> {
        let command = parse(&["stray", "start", "--shell", "/tmp/my shell", "--login"])?;

        assert!(matches!(
            command,
            Command::Start { shell: Some(path), login: true } if path == "/tmp/my shell"
        ));
        Ok(())
    }

    #[test]
    fn start_accepts_login_flag() -> Result<()> {
        assert!(matches!(
            parse(&["stray", "start", "--login"])?,
            Command::Start { login: true, .. }
        ));
        Ok(())
    }

    #[test]
    fn init_accepts_supported_shells() -> Result<()> {
        assert!(matches!(
            parse(&["stray", "init", "zsh"])?,
            Command::Init { shell: Shell::Zsh }
        ));
        assert!(matches!(
            parse(&["stray", "init", "bash"])?,
            Command::Init { shell: Shell::Bash }
        ));
        Ok(())
    }

    #[test]
    fn init_rejects_missing_or_unsupported_shells() {
        assert_eq!(
            error_kind(&["stray", "init"]),
            Some(ErrorKind::MissingRequiredArgument)
        );
        assert_eq!(
            error_kind(&["stray", "init", "fish"]),
            Some(ErrorKind::InvalidValue)
        );
    }

    #[test]
    fn signals_become_shell_style_exit_codes() {
        assert_eq!(signal_exit_code(15), ExitCode::from(143));
        assert_eq!(signal_exit_code(200), ExitCode::FAILURE);
    }
}
