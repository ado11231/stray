//! Which program is in front in the shell's terminal: the shell itself, a coding agent, or
//! some other program.

use std::os::fd::RawFd;

/// Programs found by name that are coding agents. The cat perks up when they wait for you.
const CODING_AGENTS: [&str; 2] = ["claude", "codex"];

/// The longest process name the system keeps, plus room for the end of the string.
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
        let front = match process_name(group) {
            Some(name) if CODING_AGENTS.contains(&name.as_str()) => Front::Agent,
            _ => Front::Program,
        };
        self.last = Some((group, front));
        front
    }
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
        child.kill()?;
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

    #[test]
    fn a_coding_agent_is_found_by_name() -> Result<()> {
        let folder = std::env::temp_dir().join(format!("stray-agent-{}", std::process::id()));
        std::fs::create_dir_all(&folder)?;
        let agent = folder.join("claude");
        std::fs::copy("/bin/sleep", &agent)?;
        let front = front_while_running(&agent);
        std::fs::remove_dir_all(&folder)?;
        assert_eq!(front?, Front::Agent);
        Ok(())
    }

    #[test]
    fn a_process_name_is_found() -> Result<()> {
        let mut child = Command::new("/bin/sleep").arg("5").spawn()?;
        let pid = i32::try_from(child.id())?;
        let name = process_name(pid).context("the name should be found");
        child.kill()?;
        child.wait()?;
        assert_eq!(name?, "sleep");
        Ok(())
    }

    #[test]
    fn a_missing_terminal_means_the_shell() {
        let mut foreground = Foreground::new(-1, 1);
        assert_eq!(foreground.front(), Front::Shell);
    }
}
