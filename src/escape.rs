//! Terminal control sequences, named so the code that sends them reads in plain words.
//!
//! Most sequences start with ESC (byte 0x1b). "CSI" means ESC followed by `[`.

/// The byte that starts most control sequences.
pub const ESC: u8 = 0x1b;

/// Starts a CSI sequence, such as a cursor move or a mode change.
pub const CSI: &[u8] = b"\x1b[";

/// Starts a private CSI sequence, such as a device attributes reply.
pub const CSI_PRIVATE: &[u8] = b"\x1b[?";

/// Starts a kitty graphics command or reply.
pub const GRAPHICS_START: &[u8] = b"\x1b_G";

/// Ends a string sequence, such as a graphics command.
pub const STRING_END: &[u8] = b"\x1b\\";

/// Cancels a sequence that a program left unfinished.
pub const CANCEL: &[u8] = b"\x18";

/// Asks the terminal where the cursor is. The reply looks like `ESC [ row ; column R`.
pub const CURSOR_POSITION_QUERY: &[u8] = b"\x1b[6n";

/// Asks the terminal what it supports. Every terminal answers, so the reply marks the end
/// of a group of questions.
pub const DEVICE_ATTRIBUTES_QUERY: &[u8] = b"\x1b[c";

/// Starts a message that tmux passes on to the outer terminal.
pub const TMUX_PASSTHROUGH_START: &[u8] = b"\x1bPtmux;";

/// Cursor positions count from the top of the scroll region.
pub const ORIGIN_MODE_ON: &[u8] = b"\x1b[?6h";

/// Cursor positions count from the top of the screen.
pub const ORIGIN_MODE_OFF: &[u8] = b"\x1b[?6l";

/// Ends a synchronized update, so the terminal draws again.
pub const SYNC_UPDATE_OFF: &[u8] = b"\x1b[?2026l";

/// Lets the whole screen scroll again.
pub const RESET_SCROLL_REGION: &[u8] = b"\x1b[r";

/// Clears the screen from the cursor down.
pub const CLEAR_BELOW: &[u8] = b"\x1b[J";

/// Puts the terminal back to normal when the bar goes away, in this order:
/// cancel unfinished output, end synchronized updates, turn origin mode off,
/// and let the whole screen scroll.
pub const RESTORE_TERMINAL: &[&[u8]] = &[
    CANCEL,
    STRING_END,
    SYNC_UPDATE_OFF,
    ORIGIN_MODE_OFF,
    RESET_SCROLL_REGION,
];

/// Lets only rows `1..=rows` scroll.
pub fn scroll_region(rows: u16) -> String {
    format!("\x1b[1;{rows}r")
}

/// Moves the cursor. Terminals count rows and columns from 1.
pub fn move_cursor(row: u16, column: u16) -> String {
    format!("\x1b[{row};{column}H")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_sequence_keeps_its_order() {
        assert_eq!(
            RESTORE_TERMINAL.concat(),
            b"\x18\x1b\\\x1b[?2026l\x1b[?6l\x1b[r"
        );
    }

    #[test]
    fn positions_are_written_as_terminals_expect() {
        assert_eq!(scroll_region(22), "\x1b[1;22r");
        assert_eq!(move_cursor(3, 4), "\x1b[3;4H");
    }
}
