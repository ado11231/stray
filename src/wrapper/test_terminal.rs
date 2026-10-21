//! Helpers for tests that need a real terminal of their own.

use super::launch::{ShellGuard, open_pty, spawn_shell};
use super::workers::forward_output;
use anyhow::{Context, Result, ensure};
use portable_pty::{CommandBuilder, PtySize};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Set in the copy of the test program that runs inside the test terminal.
const CHILD_TEST: &str = "STRAY_TEST_SHELL_SESSION";

/// How long an isolated test may run.
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The full name of a test function in the current module, for `--exact`.
macro_rules! test_name {
    ($name:ident) => {
        concat!(module_path!(), "::", stringify!($name))
    };
}
pub(crate) use test_name;

/// Test names given to `--exact` leave out the crate name that `module_path!` starts with.
pub fn without_crate_name(path: &str) -> &str {
    path.split_once("::").map_or(path, |(_, rest)| rest)
}

/// Runs one test again inside a fresh terminal, so raw mode and signals cannot affect
/// other tests or the terminal running `cargo test`.
///
/// Returns `true` in the parent, which should then return. Returns `false` inside the
/// fresh terminal, where the test should run.
pub fn run_in_test_terminal(test_path: &str) -> Result<bool> {
    let test_name = without_crate_name(test_path);
    if std::env::var(CHILD_TEST).as_deref() == Ok(test_name) {
        return Ok(false);
    }

    let pty = open_pty(PtySize::default())?;
    let reader = pty.master.try_clone_reader()?;
    let mut command = CommandBuilder::new(std::env::current_exe()?);
    command.args(["--exact", test_name, "--nocapture"]);
    command.env(CHILD_TEST, test_name);
    let mut test = ShellGuard {
        child: spawn_shell(&pty, command)?,
    };
    drop(pty.slave);
    let output_thread = thread::spawn(move || {
        let mut output = Vec::new();
        forward_output(reader, &mut output).map(|()| output)
    });

    let deadline = Instant::now() + TEST_TIMEOUT;
    let status = loop {
        if let Some(status) = test.child.try_wait()? {
            break status;
        }
        ensure!(Instant::now() < deadline, "isolated test timed out");
        thread::sleep(Duration::from_millis(10));
    };
    let output = output_thread
        .join()
        .map_err(|_| anyhow::anyhow!("test output thread panicked"))??;
    ensure!(
        status.success(),
        "isolated test failed: {}",
        String::from_utf8_lossy(&output)
    );
    Ok(true)
}

/// Reads the terminal settings, so tests can check that Stray put them back.
pub fn terminal_settings() -> Result<Vec<String>> {
    let output = Command::new("/bin/stty")
        .arg("-a")
        .stdin(Stdio::inherit())
        .output()
        .context("stty should start")?;
    ensure!(
        output.status.success(),
        "terminal settings should be readable"
    );
    // PENDIN tracks pending input retyping and can change when raw mode ends.
    Ok(String::from_utf8(output.stdout)?
        .split_whitespace()
        .filter(|word| !matches!(word.trim_end_matches(';'), "pendin" | "-pendin"))
        .map(str::to_owned)
        .collect())
}
