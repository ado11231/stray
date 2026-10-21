//! Small helpers for the real terminal that Stray runs in.

use anyhow::{Context, Result};
use portable_pty::PtySize;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGWINCH};
use signal_hook::iterator::Signals;
use std::ffi::{CStr, OsString};
use std::os::unix::ffi::OsStrExt;

/// Room for the terminal's path, such as /dev/ttys003.
const TTY_NAME_BUFFER_SIZE: usize = 1024;

/// Reads stdin's terminal name without depending on shell variables or PATH.
pub fn tty_name() -> Option<OsString> {
    let mut buffer = [0 as libc::c_char; TTY_NAME_BUFFER_SIZE];
    // SAFETY: the buffer is writable, and its length is passed along with it.
    let result = unsafe { libc::ttyname_r(libc::STDIN_FILENO, buffer.as_mut_ptr(), buffer.len()) };
    if result != 0 {
        return None;
    }
    // SAFETY: on success, ttyname_r wrote a null-terminated name into the buffer.
    let name = unsafe { CStr::from_ptr(buffer.as_ptr()) };
    Some(std::ffi::OsStr::from_bytes(name.to_bytes()).to_owned())
}

/// Reads the terminal's size in rows and columns.
pub fn read_size() -> Result<PtySize> {
    let (columns, rows) = crossterm::terminal::size().context("could not read terminal size")?;

    Ok(PtySize {
        rows,
        cols: columns,
        // Zero means the pixel size is unknown.
        pixel_width: 0,
        pixel_height: 0,
    })
}

/// Watches window resizes and shutdown signals.
///
/// Register this before raw mode, so shutdown signals also take the cleanup path.
pub fn watch_signals() -> Result<Signals> {
    Signals::new([SIGWINCH, SIGHUP, SIGINT, SIGQUIT, SIGTERM])
        .context("could not watch terminal signals")
}

/// Restores terminal mode when this value leaves scope.
pub struct RawModeGuard;

impl RawModeGuard {
    pub fn enable() -> Result<Self> {
        crossterm::terminal::enable_raw_mode().context("could not enable raw terminal mode")?;

        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // Cleanup must also run when a function returns an error.
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_listener_receives_window_size_signal() -> Result<()> {
        let mut signals = watch_signals()?;

        signal_hook::low_level::raise(SIGWINCH).context("resize signal should be sent")?;

        // Read pending signals without blocking if registration is broken.
        let received: Vec<_> = signals.pending().collect();
        assert_eq!(received, vec![SIGWINCH]);
        Ok(())
    }
}
