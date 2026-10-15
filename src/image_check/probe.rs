use super::{Replies, write_query};
use crate::{terminal, wrapper::TerminationSignal};
use anyhow::{Context, Result};
use std::io::{self, IsTerminal, Read};
use std::time::{Duration, Instant};

pub fn run(inside_tmux: bool) -> Result<Replies> {
    anyhow::ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "image check needs a terminal"
    );
    let mut signals = terminal::watch_signals()?;
    let _raw_mode = terminal::RawModeGuard::enable()?;
    write_query(&mut io::stdout().lock(), inside_tmux).context("could not send image check")?;

    let deadline = Instant::now() + Duration::from_millis(500);
    let mut replies = Replies::default();
    let mut input = io::stdin().lock();
    while !replies.finished && Instant::now() < deadline {
        for signal in signals.pending() {
            if signal != signal_hook::consts::SIGWINCH {
                return Err(TerminationSignal(signal).into());
            }
        }
        let mut descriptor = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        // Short polls keep termination signals responsive without a reader thread.
        let ready = unsafe { libc::poll(&mut descriptor, 1, 20) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            replies.supported = false;
            break;
        }
        if ready == 0 {
            continue;
        }
        let mut bytes = [0; 1024];
        match input.read(&mut bytes) {
            Ok(0) => break,
            Ok(count) => replies.feed(&bytes[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                replies.supported = false;
                break;
            }
        }
    }
    Ok(replies.finish())
}
