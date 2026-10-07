mod terminal;
mod wrapper;

use anyhow::{Context, Result, bail};
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Install => bail!("install is not built yet"),
        Command::Uninstall => bail!("uninstall is not built yet"),
        Command::Start { login } => {
            // Read the configured shell path.
            let shell_path = std::env::var_os("SHELL").context("SHELL is not set")?;
            let command = wrapper::build_shell_command(&shell_path, login);

            let size = terminal::read_size()?;
            let pty = wrapper::open_pty(size)?;

            // Read size assigned to PTY.
            let actual_size = pty.master.get_size().context("Could not read PTY size")?;

            // Show setup until shell launching is ready.
            bail!(
                "start is not built yet (command: {:?}, PTY: {} columns, {} rows)",
                command.get_argv(),
                actual_size.cols,
                actual_size.rows
            )
        }
        Command::Init { shell: _ } => bail!("init is not built yet"),
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
        assert!(matches!(start.command, Command::Start { login: false }));
    }

    #[test]
    fn start_accepts_login_flag() {
        let cli = Cli::try_parse_from(["stray", "start", "--login"]).unwrap();

        assert!(matches!(cli.command, Command::Start { login: true }));
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
