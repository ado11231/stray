use anyhow::{Context, Result};
use portable_pty::PtySize;

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
