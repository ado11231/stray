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
