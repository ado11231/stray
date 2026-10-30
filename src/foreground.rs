//! Which program is in front in the shell's terminal: the shell itself, a coding agent, or
//! some other program.

use std::os::fd::RawFd;

/// Programs found by name that are coding agents. The cat perks up when they wait for you.
const CODING_AGENTS: [&str; 2] = ["claude", "codex"];

/// Some programs, such as Claude Code from npm, run a file whose name ends in this.
const EXE_ENDING: &str = ".exe";

/// The longest process name the system keeps, plus room for the end of the string.
#[cfg(target_os = "macos")]
const NAME_BUFFER_SIZE: usize = 256;

/// What is in front in the shell's terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Front {
    /// The shell is waiting at the prompt.
    Shell,
    /// A coding agent such as `claude` is running.
    Agent,
    /// Any other program is running.
    Program,
}

/// Asks the shell's terminal which program is in front.
pub struct Foreground {
    /// Stray's side of the shell's terminal.
    terminal: RawFd,
    /// The shell's process ID, which is also its process group.
    shell: i32,
    /// The process group in front at the last check, and what it was, so the name is
    /// looked up only when the group changes.
    last: Option<(i32, Front)>,
}

impl Foreground {
    /// Watches `terminal`, the PTY side that Stray holds, for the shell with process `shell`.
    pub fn new(terminal: RawFd, shell: i32) -> Self {
        Self {
            terminal,
            shell,
            last: None,
        }
    }

    /// What is in front now. When the terminal cannot tell, the shell is assumed.
    pub fn front(&mut self) -> Front {
        // SAFETY: tcgetpgrp only reads the process group of an open file descriptor. On a
        // closed or wrong one it returns -1, which is handled below.
        let group = unsafe { libc::tcgetpgrp(self.terminal) };
        if group <= 0 || group == self.shell {
            return Front::Shell;
        }
        if let Some((last_group, front)) = self.last
            && last_group == group
        {
            return front;
        }
        let names = [first_argument(group), process_name(group)];
        let front = front_for(&names);
        self.last = Some((group, front));
        front
    }
}

/// What a running program is, from its names. Both the name it was started with and the
/// name of its file are checked, because they can differ: Claude Code from npm is started
/// as `claude` but its file is `claude.exe`, and the native one's file is its version.
fn front_for(names: &[Option<String>]) -> Front {
    let is_agent = names.iter().flatten().any(|name| {
        // A path such as `/usr/local/bin/claude` counts by its last part.
        let name = name.rsplit('/').next().unwrap_or(name);
        let name = name.strip_suffix(EXE_ENDING).unwrap_or(name);
        CODING_AGENTS.contains(&name)
    });
    if is_agent {
        Front::Agent
    } else {
        Front::Program
    }
}

/// The name process `pid` was started with, its first argument, such as `claude`.
#[cfg(target_os = "macos")]
fn first_argument(pid: i32) -> Option<String> {
    let mut query = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let query_length = u32::try_from(query.len()).ok()?;
    let mut size: libc::size_t = 0;
    // SAFETY: with no buffer given, sysctl only writes into `size` how big the answer is.
    let failed = unsafe {
        libc::sysctl(
            query.as_mut_ptr(),
            query_length,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if failed != 0 || size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size];
    // SAFETY: sysctl writes at most `size` bytes into `buffer`, which is that long, and sets
    // `size` to how many it wrote.
    let failed = unsafe {
        libc::sysctl(
            query.as_mut_ptr(),
            query_length,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if failed != 0 {
        return None;
    }
    // The answer holds the number of arguments, the program's path, some empty bytes,
    // then each argument. Each part ends with an empty byte.
    let after_count = buffer.get(std::mem::size_of::<i32>()..size)?;
    let path_end = after_count.iter().position(|byte| *byte == 0)?;
    let after_path = after_count.get(path_end..)?;
    let start = after_path.iter().position(|byte| *byte != 0)?;
    let first = after_path.get(start..)?.split(|byte| *byte == 0).next()?;
    Some(String::from_utf8_lossy(first).into_owned())
}

/// The name process `pid` was started with, its first argument, such as `claude`.
#[cfg(not(target_os = "macos"))]
fn first_argument(pid: i32) -> Option<String> {
    let arguments = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let first = arguments.split(|byte| *byte == 0).next()?;
    (!first.is_empty()).then(|| String::from_utf8_lossy(first).into_owned())
}

/// The short name of process `pid`, such as `vim`.
#[cfg(target_os = "macos")]
fn process_name(pid: i32) -> Option<String> {
    let mut buffer = [0u8; NAME_BUFFER_SIZE];
    let size = u32::try_from(buffer.len()).ok()?;
    // SAFETY: proc_name writes at most `size` bytes into `buffer`, which is that long, and
    // returns how many it wrote, or 0 or less on failure.
    let written = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), size) };
    let written = usize::try_from(written)
        .ok()
        .filter(|written| *written > 0)?;
    let name = buffer.get(..written)?;
    Some(String::from_utf8_lossy(name).into_owned())
}

/// The short name of process `pid`, such as `vim`.
#[cfg(not(target_os = "macos"))]
fn process_name(pid: i32) -> Option<String> {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(name.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// How long a test waits for the shell to put its program in front.
    const WAIT: Duration = Duration::from_secs(5);

    /// Runs `program` from a shell with job control in a new terminal, and returns what was
    /// in front while it ran.
    fn front_while_running(program: &std::path::Path) -> Result<Front> {
        let pty = native_pty_system().openpty(PtySize::default())?;
        let mut command = CommandBuilder::new("/bin/sh");
        let script = format!("set -m; '{}' 3", program.display());
        command.args(["-c", &script]);
        let mut child = pty.slave.spawn_command(command)?;
        drop(pty.slave);
        let shell = i32::try_from(child.process_id().context("the shell should have an ID")?)?;
        let terminal = pty
            .master
            .as_raw_fd()
            .context("the terminal should have a file")?;
        let mut foreground = Foreground::new(terminal, shell);

        let deadline = Instant::now() + WAIT;
        let mut front = foreground.front();
        while front == Front::Shell && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            front = foreground.front();
        }
        // Close the terminal first. On macOS a shell cannot finish exiting while its
        // terminal holds output that nobody reads, so waiting would hang.
        drop(pty.master);
        // Closing the terminal hangs up the shell, so it may already be gone.
        let _ = child.kill();
        child.wait()?;
        Ok(front)
    }

    #[test]
    fn a_program_in_front_is_seen() -> Result<()> {
        assert_eq!(
            front_while_running(std::path::Path::new("/bin/sleep"))?,
            Front::Program
        );
        Ok(())
    }

    fn names(first: Option<&str>, file: Option<&str>) -> [Option<String>; 2] {
        [first.map(String::from), file.map(String::from)]
    }

    #[test]
    fn coding_agents_are_found_by_either_name() {
        for (first, file) in [
            (Some("claude"), Some("claude.exe")),
            (Some("claude"), Some("2.0.14")),
            (Some("/usr/local/bin/codex"), None),
            (None, Some("claude")),
        ] {
            assert_eq!(
                front_for(&names(first, file)),
                Front::Agent,
                "{first:?} {file:?}"
            );
        }
        for (first, file) in [
            (Some("vim"), Some("vim")),
            (Some("claudette"), None),
            (None, None),
        ] {
            assert_eq!(
                front_for(&names(first, file)),
                Front::Program,
                "{first:?} {file:?}"
            );
        }
    }

    #[test]
    fn a_program_started_as_claude_is_an_agent() -> Result<()> {
        // A link named `claude` starts `sleep` with the name `claude`, as npm's link does.
        let folder = std::env::temp_dir().join(format!("stray-agent-{}", std::process::id()));
        std::fs::create_dir_all(&folder)?;
        let agent = folder.join("claude");
        std::os::unix::fs::symlink("/bin/sleep", &agent)?;
        let front = front_while_running(&agent);
        std::fs::remove_dir_all(&folder)?;
        assert_eq!(front?, Front::Agent);
        Ok(())
    }

    /// Starts `/bin/sleep` and returns what `look_up` finds for it once it has started. A
    /// new process takes a moment to become `sleep`, so this asks until it does.
    fn found_for_sleep(look_up: fn(i32) -> Option<String>, expected: &str) -> Result<String> {
        let mut child = Command::new("/bin/sleep").arg("5").spawn()?;
        let pid = i32::try_from(child.id())?;
        let deadline = Instant::now() + WAIT;
        let mut found = look_up(pid);
        while found.as_deref() != Some(expected) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            found = look_up(pid);
        }
        child.kill()?;
        child.wait()?;
        found.context("it should be found")
    }

    #[test]
    fn the_first_argument_is_found() -> Result<()> {
        assert_eq!(found_for_sleep(first_argument, "/bin/sleep")?, "/bin/sleep");
        Ok(())
    }

    #[test]
    fn a_process_name_is_found() -> Result<()> {
        assert_eq!(found_for_sleep(process_name, "sleep")?, "sleep");
        Ok(())
    }

    #[test]
    fn a_missing_terminal_means_the_shell() {
        let mut foreground = Foreground::new(-1, 1);
        assert_eq!(foreground.front(), Front::Shell);
    }
}
