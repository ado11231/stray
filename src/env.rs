//! Names of the environment variables Stray reads.

use anyhow::{Context, Result};
use std::ffi::OsString;

/// The user's login shell.
pub const SHELL: &str = "SHELL";

/// The user's home folder.
pub const HOME: &str = "HOME";

/// Where zsh looks for its settings, when it is not the home folder.
pub const ZDOTDIR: &str = "ZDOTDIR";

/// Set when running inside tmux.
pub const TMUX: &str = "TMUX";

/// For testing: the time the cat's clock starts at, such as `06:55`.
pub const CLOCK: &str = "STRAY_CLOCK";

/// For testing: how many times faster than real time the cat's clock runs.
pub const CLOCK_SPEED: &str = "STRAY_CLOCK_SPEED";

/// Reads the user's shell path.
pub fn shell() -> Result<OsString> {
    std::env::var_os(SHELL).context("SHELL is not set")
}

/// Reads the user's home folder.
pub fn home() -> Result<OsString> {
    std::env::var_os(HOME).context("HOME is not set")
}
