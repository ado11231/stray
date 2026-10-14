use anyhow::{Context, Result};
use portable_pty::PtySize;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGWINCH};
use signal_hook::iterator::Signals;

pub fn read_size() -> Result<PtySize> {
    let (columns, rows) = crossterm::terminal::size().context("could not read terminal size")?;

    Ok(PtySize {
        rows,
        cols: columns,
        // Zero means pixel size is unknown
        pixel_width: 0,
        pixel_height: 0,
    })
}

/// Registers before raw mode so shutdown signals also take the cleanup path.
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
    fn resize_listener_receives_window_size_signal() {
        let mut signals = watch_signals().expect("signal listener should register");

        signal_hook::low_level::raise(SIGWINCH).expect("resize signal should be sent");

        // Read pending signals without blocking if registration is broken.
        let received: Vec<_> = signals.pending().collect();
        assert_eq!(received, vec![SIGWINCH]);
    }
}
