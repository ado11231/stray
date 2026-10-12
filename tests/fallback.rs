#![cfg(unix)]

use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

unsafe extern "C" {
    fn setsid() -> i32;
}

struct TestShell {
    directory: PathBuf,
    path: PathBuf,
}

impl TestShell {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("stray-fallback-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory).expect("test directory should be created");
        let path = directory.join("test-shell");
        std::fs::write(
            &path,
            "#!/bin/sh\nprintf 'arg:%s\\n' \"$@\"\nIFS= read -r input\nprintf 'input:%s\\nenv:%s\\n' \"$input\" \"$STRAY_TEST_VALUE\"\nexit 7\n",
        )
        .expect("test shell should be written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("test shell should be executable");
        Self { directory, path }
    }
}

impl Drop for TestShell {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn run_without_terminal(shell: &Path, login: bool, input_bytes: &[u8]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stray"));
    command
        .arg("start")
        .env("SHELL", shell)
        .env("STRAY_TEST_VALUE", "inherited")
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

    let mut child = command.spawn().expect("Stray should start");
    let mut input = child.stdin.take().expect("stdin should be piped");
    input.write_all(input_bytes).expect("input should write");
    drop(input);
    child.wait_with_output().expect("Stray should finish")
}

#[test]
fn setup_failure_runs_direct_shell_with_original_options_and_streams() {
    let shell = TestShell::new();

    for login in [false, true] {
        let output = run_without_terminal(&shell.path, login, b"hello shell\n");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        let arguments = if login {
            "arg:-i\narg:-l\n"
        } else {
            "arg:-i\n"
        };

        assert_eq!(output.status.code(), Some(7), "{stderr}");
        assert_eq!(
            stdout,
            format!("{arguments}input:hello shell\nenv:inherited\n")
        );
        assert!(stderr.contains("starting shell directly"), "{stderr}");
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
}

#[test]
fn failed_direct_shell_launch_reports_an_error() {
    let output = run_without_terminal(Path::new("/dev/null/stray-shell"), false, b"");
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!output.status.success());
    assert!(stderr.contains("starting shell directly"), "{stderr}");
    assert!(
        stderr.contains("could not start shell directly"),
        "{stderr}"
    );
}
