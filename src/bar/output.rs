use super::shell_size;
use crate::escape;
use crate::output_boundary::OutputBoundary;
use portable_pty::PtySize;
use std::io::{self, Write};

/// Writes shell output to the terminal and keeps the bar rows out of the scroll region.
///
/// When images are not supported, bytes pass straight through and the terminal is left alone.
pub struct Output<W: Write> {
    writer: W,
    /// A copy of the shell's screen, used to put the cursor back after changing the region.
    /// It is `None` when there is no bar.
    screen: Option<vt100::Parser>,
    /// Tells when shell output is between sequences, so Stray never splits one.
    boundary: OutputBoundary,
    terminal_size: PtySize,
    /// The scroll region needs to be sent again at the next safe point.
    pending: bool,
    /// The scroll region was changed, so it must be reset on exit.
    changed_region: bool,
}

impl<W: Write> Output<W> {
    /// Starts output and reserves the bar rows when images are supported.
    ///
    /// `cursor` is the 0-based cursor position from the image check, if the terminal sent one.
    pub fn new(
        writer: W,
        size: PtySize,
        images_supported: bool,
        cursor: Option<(u16, u16)>,
    ) -> io::Result<Self> {
        let shell = shell_size(size, images_supported);
        let mut output = Self {
            writer,
            screen: images_supported
                .then(|| vt100::Parser::new(shell.rows.max(1), shell.cols.max(1), 0)),
            boundary: OutputBoundary::default(),
            terminal_size: size,
            pending: shell.rows != size.rows,
            changed_region: false,
        };
        if let Some(screen) = &mut output.screen {
            let last_row = shell.rows.saturating_sub(1);
            let last_column = shell.cols.saturating_sub(1);
            let (row, column) = cursor.unwrap_or((size.rows.saturating_sub(1), 0));
            let mut column = column.min(last_column);
            if output.pending && row >= shell.rows {
                // Make space below the existing prompt without clearing scrollback.
                output.writer.write_all(b"\r\n\r\n")?;
                column = 0;
            }
            // Start the screen copy where the real cursor is.
            let position = escape::move_cursor(row.min(last_row) + 1, column + 1);
            screen.process(position.as_bytes());
        }
        output.apply_region()?;
        Ok(output)
    }

    /// Updates the terminal size and returns the size the shell should use.
    pub fn resize(&mut self, size: PtySize) -> io::Result<PtySize> {
        let shell = shell_size(size, self.screen.is_some());
        self.terminal_size = size;
        if let Some(screen) = &mut self.screen {
            screen
                .screen_mut()
                .set_size(shell.rows.max(1), shell.cols.max(1));
            self.pending = self.changed_region || shell.rows != size.rows;
            self.apply_region()?;
        }
        Ok(shell)
    }

    /// Sends the scroll region, but only when no sequence or synchronized update is open.
    fn apply_region(&mut self) -> io::Result<()> {
        if !self.pending || !self.boundary.ready() {
            return Ok(());
        }
        let Some(parser) = &self.screen else {
            return Ok(());
        };
        let screen = parser.screen();
        let rows = screen.size().0;
        self.changed_region = true;
        self.writer.write_all(escape::ORIGIN_MODE_OFF)?;
        if rows < self.terminal_size.rows {
            // Limit scrolling to the shell rows, then clear the bar rows below them.
            self.writer
                .write_all(escape::scroll_region(rows).as_bytes())?;
            self.writer
                .write_all(escape::move_cursor(rows + 1, 1).as_bytes())?;
            self.writer.write_all(escape::CLEAR_BELOW)?;
        } else {
            self.writer.write_all(escape::RESET_SCROLL_REGION)?;
        }
        if self.boundary.origin() {
            self.writer.write_all(escape::ORIGIN_MODE_ON)?;
        }
        restore_cursor(&mut self.writer, screen)?;
        self.writer.flush()?;
        self.pending = false;
        Ok(())
    }
}

/// Puts back the shell's cursor position and text style, which changing the region resets.
fn restore_cursor(writer: &mut impl Write, screen: &vt100::Screen) -> io::Result<()> {
    writer.write_all(&screen.cursor_state_formatted())?;
    writer.write_all(&screen.attributes_formatted())
}

impl<W: Write> Write for Output<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writer.write_all(bytes)?;
        if let Some(screen) = &mut self.screen {
            screen.process(bytes);
            self.boundary.feed(bytes);
            self.apply_region()?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<W: Write> Drop for Output<W> {
    fn drop(&mut self) {
        if !self.changed_region {
            return;
        }
        // Runs on every exit path, including panics, so errors are ignored.
        for sequence in escape::RESTORE_TERMINAL {
            let _ = self.writer.write_all(sequence);
        }
        if let Some(parser) = &self.screen {
            let _ = restore_cursor(&mut self.writer, parser.screen());
        }
        let _ = self.writer.flush();
    }
}
