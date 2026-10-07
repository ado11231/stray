use anyhow::{Result, bail};
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
    #[command(hide = true)]
    Start,
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
        Command::Start => bail!("start is not built yet"),
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
        assert!(matches!(start.command, Command::Start));
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
