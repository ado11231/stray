#[derive(Default)]
pub struct OutputBoundary {
    parser: vte::Parser,
    state: State,
}

struct State {
    complete: bool,
    synchronized: bool,
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
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if byte == 0x1b || byte >= 0x80 {
                self.state.complete = false;
            }
            self.parser.advance(&mut self.state, &[byte]);
        }
    }

    pub fn ready(&self) -> bool {
        self.state.complete && !self.state.synchronized
    }

    pub fn origin(&self) -> bool {
        self.state.origin
    }
}

impl vte::Perform for State {
    fn print(&mut self, _: char) {
        self.complete = true;
    }

    fn execute(&mut self, byte: u8) {
        if matches!(byte, 0x18 | 0x1a) {
            self.complete = true;
        }
    }

    fn osc_dispatch(&mut self, _: &[&[u8]], bell_terminated: bool) {
        self.complete = bell_terminated;
    }

    fn esc_dispatch(&mut self, _: &[u8], _: bool, byte: u8) {
        self.complete = true;
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
        if !ignore && intermediates == b"?" && matches!(action, 'h' | 'l') {
            for parameter in params {
                match parameter {
                    [2026] => self.synchronized = action == 'h',
                    [6] => self.origin = action == 'h',
                    _ => {}
                }
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
            for &byte in &bytes[..bytes.len() - 1] {
                boundary.feed(&[byte]);
                assert!(!boundary.ready(), "{bytes:?}");
            }
            boundary.feed(&bytes[bytes.len() - 1..]);
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
}
