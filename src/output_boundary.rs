//! Finds safe points in shell output where Stray can send its own sequences, and notes
//! what the output did that could harm the bar.

use crate::escape::{ESC, PROMPT_MARK};

/// The first byte of a multi-byte UTF-8 character is at least this value.
const UTF8_MULTI_BYTE_START: u8 = 0x80;

/// The CAN and SUB control bytes. Both cancel an unfinished sequence.
const CANCEL_BYTES: [u8; 2] = [0x18, 0x1a];

/// The DEC private mode number for synchronized updates.
const SYNC_UPDATE_MODE: u16 = 2026;

/// The DEC private mode number for origin mode.
const ORIGIN_MODE: u16 = 6;

/// The DEC private mode numbers that switch to or from the full screen, as vim and less do.
const FULL_SCREEN_MODES: [u16; 3] = [47, 1047, 1049];

/// `ESC [ n J` with one of these clears the whole screen, or the screen and scrollback.
const CLEAR_ALL: [u16; 2] = [2, 3];

/// Watches shell output and tells when it is between sequences and characters.
#[derive(Default)]
pub struct OutputBoundary {
    parser: vte::Parser,
    state: State,
}

/// What the output did since the last check that could harm the bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    /// The screen was cleared, the terminal was reset, or the full screen was switched,
    /// so the pictures may be gone.
    pub redraw: bool,
    /// The program set the scroll region, ending at this row. 0 means the whole screen.
    pub region_bottom: Option<u16>,
    /// The lowest row a cursor move asked for, counted from 1.
    pub lowest_cursor_row: Option<u16>,
    /// The exit status from the last prompt mark: a command finished and the prompt is back.
    pub command_status: Option<i32>,
}

struct State {
    /// The last byte finished a character or a sequence.
    complete: bool,
    /// The shell started a synchronized update and has not ended it.
    synchronized: bool,
    /// The shell turned origin mode on.
    origin: bool,
    events: Events,
}

impl Default for State {
    fn default() -> Self {
        Self {
            complete: true,
            synchronized: false,
            origin: false,
            events: Events::default(),
        }
    }
}

impl OutputBoundary {
    /// Reads more shell output.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            // These bytes start something that later bytes finish.
            if byte == ESC || byte >= UTF8_MULTI_BYTE_START {
                self.state.complete = false;
            }
            self.parser.advance(&mut self.state, &[byte]);
        }
    }

    /// Returns true when Stray can write without breaking the shell's output.
    pub fn ready(&self) -> bool {
        self.state.complete && !self.state.synchronized
    }

    /// Returns true when the shell has origin mode on.
    pub fn origin(&self) -> bool {
        self.state.origin
    }

    /// Returns what the output did since the last call, and starts over.
    pub fn take_events(&mut self) -> Events {
        std::mem::take(&mut self.state.events)
    }
}

impl State {
    fn set_modes(&mut self, params: &vte::Params, enabled: bool) {
        for parameter in params {
            match parameter {
                [SYNC_UPDATE_MODE] => self.synchronized = enabled,
                [ORIGIN_MODE] => self.origin = enabled,
                [mode] if FULL_SCREEN_MODES.contains(mode) => self.events.redraw = true,
                _ => {}
            }
        }
    }

    fn move_cursor_to_row(&mut self, row: u16) {
        // Row 0 means row 1.
        let row = row.max(1);
        let lowest = self
            .events
            .lowest_cursor_row
            .map_or(row, |lowest| lowest.max(row));
        self.events.lowest_cursor_row = Some(lowest);
    }
}

/// The parameter at `index`. A missing one is 0, which means the default for most sequences.
fn param(params: &vte::Params, index: usize) -> u16 {
    params
        .iter()
        .nth(index)
        .and_then(|parameter| parameter.first())
        .copied()
        .unwrap_or(0)
}

impl vte::Perform for State {
    fn print(&mut self, _: char) {
        self.complete = true;
    }

    fn execute(&mut self, byte: u8) {
        if CANCEL_BYTES.contains(&byte) {
            self.complete = true;
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        // A string ended by ESC \ is finished only after the backslash.
        self.complete = bell_terminated;
        if let [number, status] = params
            && *number == PROMPT_MARK.as_bytes()
            && let Some(status) = std::str::from_utf8(status)
                .ok()
                .and_then(|status| status.parse().ok())
        {
            self.events.command_status = Some(status);
        }
    }

    fn esc_dispatch(&mut self, _: &[u8], _: bool, byte: u8) {
        self.complete = true;
        // ESC c resets the terminal, which also ends both modes and clears the screen.
        if byte == b'c' {
            self.synchronized = false;
            self.origin = false;
            self.events.redraw = true;
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        self.complete = true;
        if ignore {
            return;
        }
        match (intermediates, action) {
            // `ESC [ ? n h` turns a mode on and `ESC [ ? n l` turns it off.
            (b"?", 'h' | 'l') => self.set_modes(params, action == 'h'),
            (b"", 'J') if CLEAR_ALL.contains(&param(params, 0)) => self.events.redraw = true,
            // `ESC [ top ; bottom r` sets the scroll region.
            (b"", 'r') => self.events.region_bottom = Some(param(params, 1)),
            // Moves to a row and column, or to a row only.
            (b"", 'H' | 'f' | 'd') => self.move_cursor_to_row(param(params, 0)),
            // `ESC [ ! p` is a soft reset, which ends origin mode and resets the region.
            (b"!", 'p') => {
                self.origin = false;
                self.events.region_bottom = Some(0);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_marks_give_the_exit_status() {
        let mut boundary = OutputBoundary::default();
        let mark = |status: &str| format!("\x1b]{PROMPT_MARK};{status}\x07");

        boundary.feed(mark("1").as_bytes());
        assert_eq!(boundary.take_events().command_status, Some(1));

        // Split across reads, the mark is still found once it ends.
        let split = mark("130");
        let (first, second) = split.as_bytes().split_at(4);
        boundary.feed(first);
        assert_eq!(boundary.take_events().command_status, None);
        boundary.feed(second);
        assert_eq!(boundary.take_events().command_status, Some(130));

        // Other OSC strings and a mark without a number are not marks.
        boundary.feed(b"\x1b]0;7701\x07");
        boundary.feed(mark("oops").as_bytes());
        assert_eq!(boundary.take_events().command_status, None);
    }

    #[test]
    fn waits_for_complete_sequences_and_utf8_characters() {
        for bytes in [
            b"\x1b[31m".as_slice(),
            b"\x1b]0;title\x1b\\",
            b"\x1b]0;title\x07",
            b"\x1bPdata\x1b\\",
            b"\x1b_Gimage\x1b\\",
            "猫".as_bytes(),
        ] {
            let mut boundary = OutputBoundary::default();
            let (start, last) = bytes.split_at(bytes.len() - 1);
            for &byte in start {
                boundary.feed(&[byte]);
                assert!(!boundary.ready(), "{bytes:?}");
            }
            boundary.feed(last);
            assert!(boundary.ready(), "{bytes:?}");
        }
    }

    #[test]
    fn waits_until_synchronized_output_ends() {
        let mut boundary = OutputBoundary::default();
        boundary.feed(b"\x1b[?2026hhello\x1b[31m");
        assert!(!boundary.ready());
        boundary.feed(b"\x1b[?2026l");
        assert!(boundary.ready());
    }

    #[test]
    fn tracks_origin_mode_until_a_reset() {
        let mut boundary = OutputBoundary::default();
        boundary.feed(b"\x1b[?6h");
        assert!(boundary.origin());
        boundary.feed(b"\x1bc");
        assert!(!boundary.origin());
    }

    #[test]
    fn notes_what_could_harm_the_bar_until_taken() {
        let mut boundary = OutputBoundary::default();
        boundary.feed(b"hello\x1b[J\x1b[5;10r\x1b[3;1H\x1b[30d\x1b[H");
        assert_eq!(
            boundary.take_events(),
            Events {
                redraw: false,
                region_bottom: Some(10),
                lowest_cursor_row: Some(30),
                command_status: None,
            }
        );
        assert_eq!(boundary.take_events(), Events::default());

        for bytes in [
            b"\x1b[2J".as_slice(),
            b"\x1b[3J",
            b"\x1bc",
            b"\x1b[?1049h",
            b"\x1b[?47l",
        ] {
            boundary.feed(bytes);
            assert!(boundary.take_events().redraw, "{bytes:?}");
        }

        boundary.feed(b"\x1b[r");
        assert_eq!(boundary.take_events().region_bottom, Some(0));
        boundary.feed(b"\x1b[?6h\x1b[!p");
        assert!(!boundary.origin());
        assert_eq!(boundary.take_events().region_bottom, Some(0));
    }
}
