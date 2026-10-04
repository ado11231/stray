use anyhow::{Result, bail};
use clap:: {Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "stray", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show how long the cat has lived with you.
    Stats,
    Install,
    Uninstall,
    #[command(hide = true)]
    Start,
    #[command(hide = true)]
    Init { shell: Shell},
}

#[derive(Clone, Copy, ValueEnum)]
enum Shell {
    Zsh,
    Bash,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Stats => bail!("stats is not built yet"),
        Command::Install => bail!("install is not built yet"),
        Command::Uninstall => bail!("uninstall is not built yet"),
        Command::Start => bail!("start is not built yet"),
        Command::Init { shell: _ } => bail!("init is not built yet"),
    }
}