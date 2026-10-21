//! Asks the terminal whether it can show images, and reads its replies.
//!
//! Keys typed while waiting arrive mixed with the replies, so the parser keeps every byte
//! that is not a reply and hands it to the shell later.

use std::io::{self, Write};
use stray_cat::escape::{
    CSI, CSI_PRIVATE, CURSOR_POSITION_QUERY, DEVICE_ATTRIBUTES_QUERY, ESC, GRAPHICS_START,
    STRING_END, TMUX_PASSTHROUGH_START,
};

pub mod hint;
pub mod probe;

/// An image ID that other programs are unlikely to use, so replies to it are ours.
const IMAGE_ID: u32 = 3_927_641;

/// Longest reply Stray waits for. Anything longer is treated as typed keys.
const RESPONSE_LIMIT: usize = 1024;

/// The message in a graphics reply when the terminal can show images.
const SUPPORTED_MESSAGE: &[u8] = b"OK";

/// The last byte of a cursor position reply.
const CURSOR_REPLY_END: u8 = b'R';

/// The last byte of a device attributes reply.
const ATTRIBUTES_REPLY_END: u8 = b'c';

/// Asks to check, but not show, a one pixel image (`a=q` means query only).
fn graphics_query() -> String {
    format!("\x1b_Gi={IMAGE_ID},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\")
}

/// Sends the image, cursor, and device attribute questions.
pub fn write_query(output: &mut impl Write, inside_tmux: bool) -> io::Result<()> {
    let graphics = graphics_query();
    if inside_tmux {
        // Ask tmux for the pane cursor, rather than the outer terminal cursor.
        output.write_all(CURSOR_POSITION_QUERY)?;
        // tmux passes the rest on to the outer terminal.
        output.write_all(TMUX_PASSTHROUGH_START)?;
        output.write_all(&double_escapes(graphics.as_bytes()))?;
        output.write_all(&double_escapes(DEVICE_ATTRIBUTES_QUERY))?;
        output.write_all(STRING_END)?;
    } else {
        output.write_all(graphics.as_bytes())?;
        output.write_all(CURSOR_POSITION_QUERY)?;
        output.write_all(DEVICE_ATTRIBUTES_QUERY)?;
    }
    output.flush()
}

/// tmux needs every ESC inside a passthrough message written twice.
fn double_escapes(bytes: &[u8]) -> Vec<u8> {
    let mut doubled = Vec::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        if byte == ESC {
            doubled.push(ESC);
        }
        doubled.push(byte);
    }
    doubled
}

/// What the parser decided about the bytes waiting in `pending`.
enum Step {
    /// The bytes may still become a reply, so wait for more.
    Wait,
    /// A whole reply was read, or passed on to the shell as a whole.
    Done,
    /// The first byte does not start a reply, so it is a typed key.
    Key,
}

/// The terminal's replies, collected one byte at a time.
#[derive(Default)]
pub struct Replies {
    /// Bytes that might still be the start of a reply.
    pending: Vec<u8>,
    /// Bytes that were not replies, such as typed keys, kept for the shell.
    pub input: Vec<u8>,
    /// The terminal said it can show images.
    pub supported: bool,
    /// The device attributes reply came, so no more replies are expected.
    pub finished: bool,
    /// The 0-based cursor row and column, if the terminal sent them.
    pub cursor: Option<(u16, u16)>,
}

impl Replies {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.pending.push(byte);
            self.parse();
        }
    }

    /// Moves everything still pending to the shell input.
    pub fn finish(mut self) -> Self {
        self.input.append(&mut self.pending);
        self
    }

    fn parse(&mut self) {
        while !self.pending.is_empty() {
            let step =
                if GRAPHICS_START.starts_with(&self.pending) || CSI.starts_with(&self.pending) {
                    Step::Wait
                } else if self.pending.starts_with(GRAPHICS_START) {
                    self.graphics_reply()
                } else if self.pending.starts_with(CSI_PRIVATE) {
                    self.attributes_reply()
                } else if self.pending.starts_with(CSI) {
                    self.cursor_reply()
                } else {
                    Step::Key
                };
            match step {
                Step::Wait | Step::Done => return,
                Step::Key => self.input.push(self.pending.remove(0)),
            }
        }
    }

    /// Reads `ESC _ G i=<id>;<message> ESC \`.
    fn graphics_reply(&mut self) -> Step {
        if !self.pending.ends_with(STRING_END) {
            return if self.pending.len() < RESPONSE_LIMIT {
                Step::Wait
            } else {
                Step::Key
            };
        }
        let body = &self.pending[GRAPHICS_START.len()..self.pending.len() - STRING_END.len()];
        let prefix = format!("i={IMAGE_ID};");
        match body.strip_prefix(prefix.as_bytes()) {
            Some(message) => {
                // A reply after the attributes reply is too late to count.
                if !self.finished {
                    self.supported = message == SUPPORTED_MESSAGE;
                }
                self.pending.clear();
            }
            // A reply for another program's image belongs to the shell.
            None => self.input.append(&mut self.pending),
        }
        Step::Done
    }

    /// Reads `ESC [ ? <numbers> c`, the answer that ends the check.
    fn attributes_reply(&mut self) -> Step {
        let parameters = &self.pending[CSI_PRIVATE.len()..];
        if let Some((&ATTRIBUTES_REPLY_END, numbers)) = parameters.split_last()
            && is_numbers(numbers)
        {
            self.finished = true;
            self.pending.clear();
            return Step::Done;
        }
        wait_for_numbers(parameters, self.pending.len())
    }

    /// Reads `ESC [ <row> ; <column> R`. Only the first one before the check ends counts.
    fn cursor_reply(&mut self) -> Step {
        let parameters = &self.pending[CSI.len()..];
        if self.cursor.is_none()
            && !self.finished
            && let Some((&CURSOR_REPLY_END, numbers)) = parameters.split_last()
            && let Some(cursor) = parse_cursor(numbers)
        {
            self.cursor = Some(cursor);
            self.pending.clear();
            return Step::Done;
        }
        wait_for_numbers(parameters, self.pending.len())
    }
}

/// Keeps waiting while the reply so far is only numbers and is not too long.
fn wait_for_numbers(parameters: &[u8], length: usize) -> Step {
    if is_numbers(parameters) && length < RESPONSE_LIMIT {
        Step::Wait
    } else {
        Step::Key
    }
}

/// Reply parameters are digits separated by semicolons.
fn is_numbers(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|&byte| byte.is_ascii_digit() || byte == b';')
}

/// Turns a 1-based `row;column` pair into a 0-based position.
fn parse_cursor(numbers: &[u8]) -> Option<(u16, u16)> {
    let text = std::str::from_utf8(numbers).ok()?;
    let (row, column) = text.split_once(';')?;
    let row = row.parse::<u16>().ok()?.checked_sub(1)?;
    let column = column.parse::<u16>().ok()?.checked_sub(1)?;
    Some((row, column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_can_split_at_every_byte_without_losing_keys() {
        let bytes = format!("hello\x1b_Gi={IMAGE_ID};OK\x1b\\\x1b[12;34R\x1b[?62;22c世界\x1b[A\r");
        for split in 0..=bytes.len() {
            let mut replies = Replies::default();
            replies.feed(&bytes.as_bytes()[..split]);
            replies.feed(&bytes.as_bytes()[split..]);
            let replies = replies.finish();
            assert!(replies.supported && replies.finished);
            assert_eq!(replies.cursor, Some((11, 33)));
            assert_eq!(replies.input, "hello世界\x1b[A\r".as_bytes());
        }
    }

    #[test]
    fn missing_failed_and_late_graphics_replies_are_not_support() {
        for bytes in [
            "\x1b[?1;2c".to_owned(),
            format!("\x1b_Gi={IMAGE_ID};EINVAL:failed\x1b\\\x1b[?1;2c"),
            format!("\x1b[?1;2c\x1b_Gi={IMAGE_ID};OK\x1b\\"),
        ] {
            let mut replies = Replies::default();
            replies.feed(bytes.as_bytes());
            assert!(!replies.supported);
            assert!(replies.finished);
            assert!(replies.finish().input.is_empty());
        }
    }

    #[test]
    fn unrelated_sequences_and_incomplete_keys_survive() {
        let bytes = b"\x1b_Gi=12;OK\x1b\\\x1b[?25h\x1b[";
        let mut replies = Replies::default();
        replies.feed(bytes);
        assert!(!replies.supported && !replies.finished);
        assert_eq!(replies.finish().input, bytes);
    }

    #[test]
    fn success_requires_a_complete_reply_and_an_exact_ok() {
        for message in ["OKAY\x1b\\", "OK", "OK\x1b"] {
            let mut replies = Replies::default();
            replies.feed(format!("\x1b_Gi={IMAGE_ID};{message}").as_bytes());
            assert!(!replies.supported);
        }
        let mut replies = Replies::default();
        replies.feed(format!("\x1b_Gi={IMAGE_ID};OK\x1b\\").as_bytes());
        assert!(replies.supported);
        assert!(!replies.finished);
        assert!(replies.finish().input.is_empty());
    }

    #[test]
    fn an_unterminated_response_cannot_grow_the_pending_buffer_forever() {
        let mut replies = Replies::default();
        let mut bytes = b"\x1b_G".to_vec();
        bytes.extend(vec![b'x'; RESPONSE_LIMIT * 2]);
        replies.feed(&bytes);
        assert!(replies.pending.len() < RESPONSE_LIMIT);
        assert_eq!(replies.finish().input, bytes);
    }

    #[test]
    fn a_second_cursor_reply_is_kept_as_input() {
        let mut replies = Replies::default();
        replies.feed(b"\x1b[1;2R\x1b[3;4R");
        assert_eq!(replies.cursor, Some((0, 1)));
        assert_eq!(replies.finish().input, b"\x1b[3;4R");
    }

    #[test]
    fn query_checks_images_and_asks_tmux_for_the_pane_cursor() -> io::Result<()> {
        let mut direct = Vec::new();
        write_query(&mut direct, false)?;
        assert!(direct.windows(3).any(|bytes| bytes == b"a=q"));
        assert!(direct.ends_with(b"\x1b\\\x1b[6n\x1b[c"));

        let mut tmux = Vec::new();
        write_query(&mut tmux, true)?;
        assert!(tmux.starts_with(b"\x1b[6n\x1bPtmux;"));
        let expected = double_escapes(&[graphics_query().as_bytes(), b"\x1b[c"].concat());
        assert_eq!(&tmux[11..tmux.len() - 2], expected);
        Ok(())
    }
}
