use anyhow::{Context, Result};
use portable_pty::PtySize;
use signal_hook::consts::SIGWINCH;
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

/// Registers a listener for terminal window size changes.
pub fn watch_resize() -> Result<Signals> {
    Signals::new([SIGWINCH]).context("could not watch terminal resizes")
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
        let mut signals = watch_resize().expect("resize listener should register");

        signal_hook::low_level::raise(SIGWINCH).expect("resize signal should be sent");

        // Read pending signals without blocking if registration is broken.
        let received: Vec<_> = signals.pending().collect();
        assert_eq!(received, vec![SIGWINCH]);
    }
}
