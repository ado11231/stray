//! Finds safe points in shell output where Stray can send its own sequences.

use crate::escape::ESC;

/// The first byte of a multi-byte UTF-8 character is at least this value.
const UTF8_MULTI_BYTE_START: u8 = 0x80;

/// The CAN and SUB control bytes. Both cancel an unfinished sequence.
const CANCEL_BYTES: [u8; 2] = [0x18, 0x1a];

/// The DEC private mode number for synchronized updates.
const SYNC_UPDATE_MODE: u16 = 2026;

/// The DEC private mode number for origin mode.
const ORIGIN_MODE: u16 = 6;

/// Watches shell output and tells when it is between sequences and characters.
#[derive(Default)]
pub struct OutputBoundary {
    parser: vte::Parser,
    state: State,
}

struct State {
    /// The last byte finished a character or a sequence.
    complete: bool,
    /// The shell started a synchronized update and has not ended it.
    synchronized: bool,
    /// The shell turned origin mode on.
    origin: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            complete: true,
            synchronized: false,
            origin: false,
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

    fn osc_dispatch(&mut self, _: &[&[u8]], bell_terminated: bool) {
        // A string ended by ESC \ is finished only after the backslash.
        self.complete = bell_terminated;
    }

    fn esc_dispatch(&mut self, _: &[u8], _: bool, byte: u8) {
        self.complete = true;
        // ESC c resets the terminal, which also ends both modes.
        if byte == b'c' {
            self.synchronized = false;
            self.origin = false;
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
        // `ESC [ ? n h` turns a mode on and `ESC [ ? n l` turns it off.
        if ignore || intermediates != b"?" || !matches!(action, 'h' | 'l') {
            return;
        }
        let enabled = action == 'h';
        for parameter in params {
            match parameter {
                [SYNC_UPDATE_MODE] => self.synchronized = enabled,
                [ORIGIN_MODE] => self.origin = enabled,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
