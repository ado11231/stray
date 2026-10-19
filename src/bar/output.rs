use super::shell_size;
use crate::output_boundary::OutputBoundary;
use portable_pty::PtySize;
use std::io::{self, Write};

pub struct Output<W: Write> {
    writer: W,
    screen: Option<vt100::Parser>,
    boundary: OutputBoundary,
    terminal_size: PtySize,
    pending: bool,
    changed_region: bool,
}

impl<W: Write> Output<W> {
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
            let (row, column) = cursor.unwrap_or((size.rows.saturating_sub(1), 0));
            let mut column = column.min(shell.cols.saturating_sub(1));
            if output.pending && row >= shell.rows {
                // Make space below the existing prompt without clearing scrollback.
                output.writer.write_all(b"\r\n\r\n")?;
                column = 0;
            }
            let position = format!(
                "\x1b[{};{}H",
                row.min(shell.rows.saturating_sub(1)) + 1,
                column + 1
            );
            screen.process(position.as_bytes());
        }
        output.apply_region()?;
        Ok(output)
    }

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

    fn apply_region(&mut self) -> io::Result<()> {
        if !self.pending || !self.boundary.ready() {
            return Ok(());
        }
        let screen = self.screen.as_ref().unwrap().screen();
        let rows = screen.size().0;
        self.changed_region = true;
        self.writer.write_all(b"\x1b[?6l")?;
        if rows < self.terminal_size.rows {
            write!(self.writer, "\x1b[1;{rows}r\x1b[{};1H\x1b[J", rows + 1)?;
        } else {
            self.writer.write_all(b"\x1b[r")?;
        }
        if self.boundary.origin() {
            self.writer.write_all(b"\x1b[?6h")?;
        }
        self.writer.write_all(&screen.cursor_state_formatted())?;
        self.writer.write_all(&screen.attributes_formatted())?;
        self.writer.flush()?;
        self.pending = false;
        Ok(())
    }
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
        if self.changed_region {
            // Cancel unfinished output before restoring the terminal on every exit path.
            let _ = self
                .writer
                .write_all(b"\x18\x1b\\\x1b[?2026l\x1b[?6l\x1b[r");
            if let Some(screen) = &self.screen {
                let _ = self
                    .writer
                    .write_all(&screen.screen().cursor_state_formatted());
                let _ = self
                    .writer
                    .write_all(&screen.screen().attributes_formatted());
            }
            let _ = self.writer.flush();
        }
    }
}
