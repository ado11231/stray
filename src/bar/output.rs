use super::scene::{CellSize, Scene};
use super::shell_size;
use crate::escape;
use crate::frames::Frames;
use crate::output_boundary::OutputBoundary;
use portable_pty::PtySize;
use std::io::{self, Write};
use std::num::NonZeroU16;

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
    /// The cat and its things. `None` when nothing is drawn, such as inside tmux for now,
    /// where a picture would not follow its pane.
    scene: Option<Scene>,
    /// The size of a character cell in pixels, when the terminal reports it.
    cell: Option<CellSize>,
}

impl<W: Write> Output<W> {
    /// Starts output and reserves the bar rows when images are supported.
    ///
    /// `cursor` is the 0-based cursor position from the image check, if the terminal sent one.
    pub fn new(
        writer: W,
        size: PtySize,
        images_supported: bool,
        inside_tmux: bool,
        cursor: Option<(u16, u16)>,
    ) -> io::Result<Self> {
        let shell = shell_size(size, images_supported);
        let mut output = Self {
            writer,
            screen: images_supported
                .then(|| vt100::Parser::new(at_least_one(shell.rows), at_least_one(shell.cols), 0)),
            boundary: OutputBoundary::default(),
            terminal_size: size,
            pending: shell.rows != size.rows,
            changed_region: false,
            scene: (images_supported && !inside_tmux).then(|| Scene::new(Frames::new(None))),
            cell: cell_size(size),
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
        self.cell = cell_size(size);
        if let Some(screen) = &mut self.screen {
            screen.set_size(at_least_one(shell.rows), at_least_one(shell.cols));
            self.pending = self.changed_region || shell.rows != size.rows;
            self.apply_region()?;
        }
        Ok(shell)
    }

    /// Fixes what the shell's output did to the bar, once the output is at a safe point.
    fn repair(&mut self) -> io::Result<()> {
        if !self.boundary.ready() {
            return Ok(());
        }
        let events = self.boundary.take_events();
        let Some(parser) = &self.screen else {
            return Ok(());
        };
        let screen = parser.screen();
        let rows = screen.size().0.get();
        if rows < self.terminal_size.rows {
            // A scroll region past the shell rows would let the bar scroll with the text.
            let region_reaches_bar = events
                .region_bottom
                .is_some_and(|bottom| bottom == 0 || bottom > rows);
            // The program thinks the screen ends at the shell rows, but the terminal still
            // lets the cursor go lower. With origin mode on, the region keeps it out.
            let cursor_in_bar =
                !self.boundary.origin() && events.lowest_cursor_row.is_some_and(|row| row > rows);
            if events.redraw || region_reaches_bar {
                self.pending = true;
            } else if cursor_in_bar {
                // The screen copy has the same size as the shell, so its cursor stayed above
                // the bar.
                restore_cursor(&mut self.writer, screen)?;
                self.writer.flush()?;
            }
        }
        self.apply_region()
    }

    /// Sends the scroll region and draws the bar, but only when no sequence or synchronized
    /// update is open.
    fn apply_region(&mut self) -> io::Result<()> {
        if !self.pending || !self.boundary.ready() {
            return Ok(());
        }
        let Some(parser) = &self.screen else {
            return Ok(());
        };
        let screen = parser.screen();
        let rows = screen.size().0.get();
        self.changed_region = true;
        self.writer.write_all(escape::ORIGIN_MODE_OFF)?;
        if rows < self.terminal_size.rows {
            // Limit scrolling to the shell rows, then clear the bar rows below them.
            self.writer
                .write_all(escape::scroll_region(rows).as_bytes())?;
            self.writer
                .write_all(escape::move_cursor(rows + 1, 1).as_bytes())?;
            self.writer.write_all(escape::CLEAR_BELOW)?;
            if let Some(scene) = &mut self.scene {
                scene.draw(&mut self.writer, rows + 1, self.cell)?;
            }
        } else {
            self.writer.write_all(escape::RESET_SCROLL_REGION)?;
            // The window is too short for the bar, so the cat goes away until it grows.
            if let Some(scene) = &mut self.scene {
                scene.clear(&mut self.writer)?;
            }
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
    writer.write_all(screen.cursor_state_formatted().as_bytes())?;
    writer.write_all(screen.attributes_formatted().as_bytes())
}

/// The size of a character cell, when the terminal reports the window's size in pixels.
fn cell_size(size: PtySize) -> Option<CellSize> {
    CellSize::from_window(size.cols, size.rows, size.pixel_width, size.pixel_height)
}

/// The screen copy needs at least one row and column, even in a window with none.
fn at_least_one(size: u16) -> NonZeroU16 {
    NonZeroU16::new(size).unwrap_or(NonZeroU16::MIN)
}

impl<W: Write> Write for Output<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writer.write_all(bytes)?;
        if let Some(screen) = &mut self.screen {
            screen.process(bytes);
            self.boundary.feed(bytes);
            self.repair()?;
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
        if let Some(scene) = &mut self.scene {
            let _ = scene.clear(&mut self.writer);
        }
        if let Some(parser) = &self.screen {
            let _ = restore_cursor(&mut self.writer, parser.screen());
        }
        let _ = self.writer.flush();
    }
}
