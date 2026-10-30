//! Asks the terminal whether it can show images, and reads its replies.
//!
//! Keys typed while waiting arrive mixed with the replies, so the parser keeps every byte
//! that is not a reply and hands it to the shell later.

use std::io::{self, Write};
use stray_cat::escape::{
    BACKGROUND_COLOR_QUERY, BELL, CSI, CSI_PRIVATE, CURSOR_POSITION_QUERY, DEVICE_ATTRIBUTES_QUERY,
    GRAPHICS_START, OSC, STRING_END, TEXT_COLOR_QUERY,
};
use stray_cat::frames::Palette;
use stray_cat::image::tmux_passthrough;

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

/// The OSC numbers of the text and background colors.
const TEXT_COLOR: &[u8] = b"10";
const BACKGROUND_COLOR: &[u8] = b"11";

/// Color replies start their value with this, then give red, green, and blue in hex.
const RGB_PREFIX: &[u8] = b"rgb:";

/// A color channel in a reply has at most this many hex digits.
const MAX_HEX_DIGITS: usize = 4;

/// Color channels are written in hex.
const HEX: u32 = 16;

/// Each hex digit holds 4 bits.
const BITS_PER_HEX_DIGIT: usize = 4;

/// Asks to check, but not show, a one pixel image (`a=q` means query only).
fn graphics_query() -> String {
    format!("\x1b_Gi={IMAGE_ID},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\")
}

/// Sends the image, cursor, color, and device attribute questions.
pub fn write_query(output: &mut impl Write, inside_tmux: bool) -> io::Result<()> {
    let graphics = graphics_query();
    if inside_tmux {
        // Ask tmux for the pane cursor and colors, rather than the outer terminal's.
        output.write_all(CURSOR_POSITION_QUERY)?;
        output.write_all(TEXT_COLOR_QUERY)?;
        output.write_all(BACKGROUND_COLOR_QUERY)?;
        // tmux passes the rest on to the outer terminal.
        let questions = [graphics.as_bytes(), DEVICE_ATTRIBUTES_QUERY].concat();
        output.write_all(&tmux_passthrough(&questions))?;
    } else {
        output.write_all(graphics.as_bytes())?;
        output.write_all(CURSOR_POSITION_QUERY)?;
        output.write_all(TEXT_COLOR_QUERY)?;
        output.write_all(BACKGROUND_COLOR_QUERY)?;
        output.write_all(DEVICE_ATTRIBUTES_QUERY)?;
    }
    output.flush()
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
    /// The text color, if the terminal sent it.
    text: Option<[u8; 3]>,
    /// The background color, if the terminal sent it.
    background: Option<[u8; 3]>,
}

impl Replies {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.pending.push(byte);
            self.parse();
        }
    }

    /// The terminal's colors, when it sent both.
    pub fn palette(&self) -> Option<Palette> {
        Some(Palette {
            text: self.text?,
            background: self.background?,
        })
    }

    /// Moves everything still pending to the shell input.
    pub fn finish(mut self) -> Self {
        self.input.append(&mut self.pending);
        self
    }

    fn parse(&mut self) {
        while !self.pending.is_empty() {
            let step = if GRAPHICS_START.starts_with(&self.pending)
                || CSI.starts_with(&self.pending)
                || OSC.starts_with(&self.pending)
            {
                Step::Wait
            } else if self.pending.starts_with(GRAPHICS_START) {
                self.graphics_reply()
            } else if self.pending.starts_with(OSC) {
                self.color_reply()
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

    /// Reads `ESC ] 10 ; rgb:<red>/<green>/<blue>`, or 11 for the background, ended by
    /// `ESC \\` or BEL.
    fn color_reply(&mut self) -> Step {
        let body = if let Some(body) = self.pending.strip_suffix(STRING_END) {
            body
        } else if let Some(body) = self.pending.strip_suffix(&[BELL]) {
            body
        } else {
            return if self.pending.len() < RESPONSE_LIMIT {
                Step::Wait
            } else {
                Step::Key
            };
        };
        let reply = body[OSC.len()..]
            .split(|&byte| byte == b';')
            .collect::<Vec<_>>();
        let color = match reply.as_slice() {
            [number @ (TEXT_COLOR | BACKGROUND_COLOR), value] => {
                parse_color(value).map(|color| (*number, color))
            }
            _ => None,
        };
        let Some((number, color)) = color else {
            // Any other OSC sequence belongs to the shell.
            self.input.append(&mut self.pending);
            return Step::Done;
        };
        // A reply after the attributes reply is too late to count.
        if !self.finished {
            if number == TEXT_COLOR {
                self.text = Some(color);
            } else {
                self.background = Some(color);
            }
        }
        self.pending.clear();
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

/// Reads `rgb:<red>/<green>/<blue>`, where each channel has 1 to 4 hex digits.
fn parse_color(value: &[u8]) -> Option<[u8; 3]> {
    let channels = value.strip_prefix(RGB_PREFIX)?;
    let mut color = [0; 3];
    let mut parts = channels.split(|&byte| byte == b'/');
    for channel in &mut color {
        *channel = parse_channel(parts.next()?)?;
    }
    parts.next().is_none().then_some(color)
}

/// Scales a channel of 1 to 4 hex digits to a byte, so `f`, `ff`, and `ffff` are all 255.
fn parse_channel(digits: &[u8]) -> Option<u8> {
    if digits.is_empty() || digits.len() > MAX_HEX_DIGITS {
        return None;
    }
    let value = u32::from_str_radix(std::str::from_utf8(digits).ok()?, HEX).ok()?;
    let max = (1 << (BITS_PER_HEX_DIGIT * digits.len())) - 1;
    let byte_max = u32::from(u8::MAX);
    u8::try_from((value * byte_max + max / 2) / max).ok()
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
    fn color_replies_make_a_palette_with_either_ending() {
        let bytes = b"\x1b]10;rgb:ffff/8080/0000\x1b\\x\x1b]11;rgb:1/22/333\x07\x1b[?1c";
        for split in 0..=bytes.len() {
            let mut replies = Replies::default();
            replies.feed(&bytes[..split]);
            replies.feed(&bytes[split..]);
            assert_eq!(
                replies.palette(),
                Some(Palette {
                    text: [255, 128, 0],
                    background: [17, 34, 51],
                })
            );
            assert_eq!(replies.finish().input, b"x");
        }
    }

    #[test]
    fn missing_bad_and_late_colors_give_no_palette() {
        for bytes in [
            b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b[?1c".as_slice(),
            b"\x1b]10;rgb:ff/ff\x1b\\\x1b]11;rgb:0/0/0\x1b\\\x1b[?1c",
            b"\x1b]10;rgb:fffff/0/0\x1b\\\x1b]11;rgb:0/0/0\x1b\\\x1b[?1c",
            b"\x1b]10;rgb:f/f/f\x1b\\\x1b[?1c\x1b]11;rgb:0/0/0\x1b\\",
        ] {
            let mut replies = Replies::default();
            replies.feed(bytes);
            assert_eq!(replies.palette(), None, "{bytes:?}");
        }
    }

    #[test]
    fn other_osc_sequences_are_kept_as_input() {
        let bytes = b"\x1b]0;title\x07\x1b]12;rgb:0/0/0\x1b\\";
        let mut replies = Replies::default();
        replies.feed(bytes);
        assert_eq!(replies.palette(), None);
        assert_eq!(replies.finish().input, bytes);
    }

    #[test]
    fn query_checks_images_and_asks_tmux_for_the_pane_cursor() -> io::Result<()> {
        let mut direct = Vec::new();
        write_query(&mut direct, false)?;
        assert!(direct.windows(3).any(|bytes| bytes == b"a=q"));
        let colors = [TEXT_COLOR_QUERY, BACKGROUND_COLOR_QUERY].concat();
        let after_graphics = [
            STRING_END,
            CURSOR_POSITION_QUERY,
            &colors,
            DEVICE_ATTRIBUTES_QUERY,
        ]
        .concat();
        assert!(direct.ends_with(&after_graphics));

        let mut tmux = Vec::new();
        write_query(&mut tmux, true)?;
        let questions = [graphics_query().as_bytes(), DEVICE_ATTRIBUTES_QUERY].concat();
        assert_eq!(
            tmux,
            [
                CURSOR_POSITION_QUERY,
                &colors,
                &tmux_passthrough(&questions)
            ]
            .concat()
        );
        Ok(())
    }
}
