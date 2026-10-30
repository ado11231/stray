//! Tests that Stray runs the shell directly when it cannot wrap it.

#![cfg(unix)]

use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Shown when Stray cannot wrap the shell and runs it directly instead.
const FALLBACK_MESSAGE: &str = "starting shell directly";

/// A path that can never be run, because /dev/null is a file and not a folder.
const MISSING_SHELL: &str = "/dev/null/stray-shell";

/// Prints its arguments, one line of input, and two environment values, then exits with 7.
const TEST_SHELL_SCRIPT: &str = "#!/bin/sh\nprintf 'arg:%s\\n' \"$@\"\nIFS= read -r input\nprintf 'input:%s\\nenv:%s\\nmark:%s\\n' \"$input\" \"$STRAY_TEST_VALUE\" \"${STRAY_TTY-unset}\"\nexit 7\n";

unsafe extern "C" {
    fn setsid() -> i32;
}

/// A small shell script in its own temporary folder, removed when the test ends.
struct TestShell {
    directory: PathBuf,
    path: PathBuf,
}

impl TestShell {
    fn new() -> Result<Self> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory =
            std::env::temp_dir().join(format!("stray-fallback-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let path = directory.join("test-shell");
        std::fs::write(&path, TEST_SHELL_SCRIPT)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        Ok(Self { directory, path })
    }
}

impl Drop for TestShell {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

/// Runs `stray start` with no terminal at all, so it must fall back to the shell.
fn run_without_terminal(shell: &Path, login: bool, input_bytes: &[u8]) -> Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stray"));
    command
        .arg("start")
        .env("SHELL", shell)
        .env("STRAY_TEST_VALUE", "inherited")
        .env("STRAY_TTY", "/dev/stale-terminal")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if login {
        command.arg("--login");
    }

    // Detach from any controlling terminal so setup fails even when cargo has one.
    unsafe {
        command.pre_exec(|| {
            if setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = command.spawn()?;
    let mut input = child.stdin.take().ok_or("stdin should be piped")?;
    input.write_all(input_bytes)?;
    drop(input);
    Ok(child.wait_with_output()?)
}

#[test]
fn setup_failure_runs_direct_shell_with_original_options_and_streams() -> Result {
    let shell = TestShell::new()?;

    for login in [false, true] {
        let output = run_without_terminal(&shell.path, login, b"hello shell\n")?;
        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        let arguments = if login {
            "arg:-i\narg:-l\n"
        } else {
            "arg:-i\n"
        };

        assert_eq!(output.status.code(), Some(7), "{stderr}");
        assert_eq!(
            stdout,
            format!("{arguments}input:hello shell\nenv:inherited\nmark:unset\n")
        );
        assert!(stderr.contains(FALLBACK_MESSAGE), "{stderr}");
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
    Ok(())
}

#[test]
fn failed_direct_shell_launch_reports_an_error() -> Result {
    let output = run_without_terminal(Path::new(MISSING_SHELL), false, b"")?;
    let stderr = String::from_utf8(output.stderr)?;

    assert!(!output.status.success());
    assert!(stderr.contains(FALLBACK_MESSAGE), "{stderr}");
    assert!(
        stderr.contains("could not start shell directly"),
        "{stderr}"
    );
    Ok(())
}
