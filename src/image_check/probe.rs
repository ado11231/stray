//! Sends the image check to the terminal and waits briefly for the answers.

use super::{Replies, write_query};
use crate::{terminal, wrapper::TerminationSignal};
use anyhow::{Context, Result};
use std::io::{self, IsTerminal, Read};
use std::time::{Duration, Instant};

/// How long to wait for the terminal to answer. Terminals that never answer start without a cat.
const REPLY_TIMEOUT: Duration = Duration::from_millis(500);

/// How long each wait for input lasts, in milliseconds, before signals are checked again.
const POLL_INTERVAL_MS: i32 = 20;

/// How many bytes are read from the terminal at a time.
const READ_BUFFER_SIZE: usize = 1024;

/// Asks the terminal about image support and returns its replies, plus any typed keys.
pub fn run(inside_tmux: bool) -> Result<Replies> {
    anyhow::ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "image check needs a terminal"
    );
    let mut signals = terminal::watch_signals()?;
    let _raw_mode = terminal::RawModeGuard::enable()?;
    write_query(&mut io::stdout().lock(), inside_tmux).context("could not send image check")?;

    let deadline = Instant::now() + REPLY_TIMEOUT;
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
        // SAFETY: `descriptor` is a valid pollfd, and the count of 1 matches it.
        let ready = unsafe { libc::poll(&mut descriptor, 1, POLL_INTERVAL_MS) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            // Polling failed, so treat images as unsupported.
            replies.supported = false;
            break;
        }
        if ready == 0 {
            continue;
        }
        let mut bytes = [0; READ_BUFFER_SIZE];
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
