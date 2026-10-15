use std::io::{self, Write};

pub mod hint;
pub mod probe;

const IMAGE_ID: u32 = 3_927_641;
const RESPONSE_LIMIT: usize = 1024;

pub fn write_query(output: &mut impl Write, inside_tmux: bool) -> io::Result<()> {
    let query = format!("\x1b_Gi={IMAGE_ID},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c");
    if inside_tmux {
        output.write_all(b"\x1bPtmux;")?;
        output.write_all(query.replace('\x1b', "\x1b\x1b").as_bytes())?;
        output.write_all(b"\x1b\\")?;
    } else {
        output.write_all(query.as_bytes())?;
    }
    output.flush()
}

#[derive(Default)]
pub struct Replies {
    pending: Vec<u8>,
    pub input: Vec<u8>,
    pub supported: bool,
    pub finished: bool,
}

impl Replies {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.pending.push(byte);
            self.parse();
        }
    }

    fn parse(&mut self) {
        while !self.pending.is_empty() {
            let graphics = b"\x1b_G";
            let attributes = b"\x1b[?";
            if graphics.starts_with(&self.pending) || attributes.starts_with(&self.pending) {
                return;
            }
            if self.pending.starts_with(graphics) {
                if self.pending.ends_with(b"\x1b\\") {
                    let reply = &self.pending[3..self.pending.len() - 2];
                    let prefix = format!("i={IMAGE_ID};");
                    if let Some(message) = reply.strip_prefix(prefix.as_bytes()) {
                        if !self.finished {
                            self.supported = message == b"OK";
                        }
                        self.pending.clear();
                    } else {
                        self.input.append(&mut self.pending);
                    }
                    return;
                }
                if self.pending.len() < RESPONSE_LIMIT {
                    return;
                }
            } else if self.pending.starts_with(attributes) {
                let parameters = &self.pending[3..];
                if parameters.last() == Some(&b'c')
                    && parameters[..parameters.len() - 1]
                        .iter()
                        .all(|byte| byte.is_ascii_digit() || *byte == b';')
                {
                    self.finished = true;
                    self.pending.clear();
                    return;
                }
                if parameters.iter().all(|b| b.is_ascii_digit() || *b == b';')
                    && self.pending.len() < RESPONSE_LIMIT
                {
                    return;
                }
            }
            self.input.push(self.pending.remove(0));
        }
    }

    pub fn finish(mut self) -> Self {
        self.input.append(&mut self.pending);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_can_split_at_every_byte_without_losing_keys() {
        let bytes = format!("hello\x1b_Gi={IMAGE_ID};OK\x1b\\\x1b[?62;22c世界\x1b[A\r");
        for split in 0..=bytes.len() {
            let mut replies = Replies::default();
            replies.feed(&bytes.as_bytes()[..split]);
            replies.feed(&bytes.as_bytes()[split..]);
            let replies = replies.finish();
            assert!(replies.supported && replies.finished);
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
    fn query_is_check_only_and_tmux_escapes_both_questions() {
        let mut direct = Vec::new();
        write_query(&mut direct, false).unwrap();
        assert!(direct.windows(3).any(|bytes| bytes == b"a=q"));
        assert!(direct.ends_with(b"\x1b\\\x1b[c"));
        let mut tmux = Vec::new();
        write_query(&mut tmux, true).unwrap();
        assert!(tmux.starts_with(b"\x1bPtmux;"));
        assert_eq!(
            &tmux[7..tmux.len() - 2],
            String::from_utf8(direct)
                .unwrap()
                .replace('\x1b', "\x1b\x1b")
                .as_bytes()
        );
    }
}
