//! The bar: two rows at the bottom of the terminal where the cat will live.

use crate::frames::Palette;
use portable_pty::PtySize;

mod output;
mod scene;
pub use output::Output;

/// Rows kept free at the bottom of the terminal for the cat.
const BAR_ROWS: u16 = 2;

/// Shorter terminals keep every row for the shell and show no bar.
pub const MIN_TERMINAL_ROWS: u16 = 10;

/// What the image check found out about the terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Setup {
    pub images_supported: bool,
    /// Inside tmux nothing is drawn yet, because a picture would not follow its pane.
    pub inside_tmux: bool,
    /// The 0-based cursor row and column, if the terminal sent them.
    pub cursor: Option<(u16, u16)>,
    /// The text and background colors, if the terminal sent both.
    pub palette: Option<Palette>,
}

/// Calculates the shell area, leaving room for the bar when it can be shown.
pub fn shell_size(terminal_size: PtySize, images_supported: bool) -> PtySize {
    if !images_supported || terminal_size.rows < MIN_TERMINAL_ROWS {
        return terminal_size;
    }

    let rows = terminal_size.rows - BAR_ROWS;
    // Keep the pixel height consistent with the smaller number of shell rows.
    let pixel_height =
        u32::from(terminal_size.pixel_height) * u32::from(rows) / u32::from(terminal_size.rows);

    PtySize {
        rows,
        pixel_height: pixel_height as u16,
        ..terminal_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_terminals_leave_two_rows_for_the_bar() {
        for (rows, expected_rows) in [(10, 8), (24, 22), (40, 38)] {
            let terminal = PtySize {
                rows,
                cols: 80,
                pixel_width: 800,
                pixel_height: rows * 20,
            };

            let shell = shell_size(terminal, true);

            assert_eq!(shell.rows, expected_rows);
            assert_eq!(shell.cols, 80);
            assert_eq!(shell.pixel_width, 800);
            assert_eq!(shell.pixel_height, expected_rows * 20);
        }
    }

    #[test]
    fn short_terminals_keep_the_full_shell_area() {
        for rows in 0..10 {
            let terminal = PtySize {
                rows,
                cols: 80,
                pixel_width: 800,
                pixel_height: rows * 20,
            };

            assert_eq!(shell_size(terminal, true), terminal);
        }
    }

    #[test]
    fn unsupported_terminals_keep_the_full_shell_area() {
        for rows in [0, 9, 10, 24, u16::MAX] {
            let terminal = PtySize {
                rows,
                cols: 120,
                pixel_width: 1200,
                pixel_height: 900,
            };

            assert_eq!(shell_size(terminal, false), terminal);
        }
    }

    #[test]
    fn unknown_pixel_dimensions_stay_unknown() {
        let terminal = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };

        let shell = shell_size(terminal, true);

        assert_eq!(shell.rows, 22);
        assert_eq!(shell.pixel_width, 0);
        assert_eq!(shell.pixel_height, 0);
    }

    #[test]
    fn maximum_dimensions_do_not_overflow() {
        let terminal = PtySize {
            rows: u16::MAX,
            cols: u16::MAX,
            pixel_width: u16::MAX,
            pixel_height: u16::MAX,
        };

        let shell = shell_size(terminal, true);

        assert_eq!(shell.rows, u16::MAX - 2);
        assert_eq!(shell.cols, u16::MAX);
        assert_eq!(shell.pixel_width, u16::MAX);
        assert_eq!(shell.pixel_height, u16::MAX - 2);
    }
}
