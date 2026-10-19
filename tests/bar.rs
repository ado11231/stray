use portable_pty::PtySize;
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;
use stray_cat::bar::Output;

#[derive(Clone, Default)]
struct Writer(Rc<RefCell<Vec<u8>>>);

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Writer {
    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

fn size(rows: u16) -> PtySize {
    PtySize {
        rows,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[test]
fn scrolling_leaves_the_bar_empty_and_preserves_the_programs_saved_cursor() {
    let writer = Writer::default();
    let mut terminal = vt100::Parser::new(24, 80, 0);
    terminal.process(b"\x1b[6;7H\x1b7\x1b[8;9H");
    let mut output = Output::new(writer.clone(), size(24), true, Some((7, 8))).unwrap();
    terminal.process(&writer.take());
    assert_eq!(terminal.screen().cursor_position(), (7, 8));

    output.write_all(b"\x1b[22;1Hone\r\ntwo\r\nthree").unwrap();
    terminal.process(&writer.take());
    let rows: Vec<_> = terminal.screen().rows(0, 80).collect();
    assert_eq!(&rows[19..], ["one", "two", "three", "", ""]);

    output.resize(size(24)).unwrap();
    terminal.process(&writer.take());
    terminal.process(b"\x1b8");
    assert_eq!(terminal.screen().cursor_position(), (5, 6));
}

#[test]
fn resizing_across_the_height_boundary_updates_the_region() {
    let writer = Writer::default();
    let mut output = Output::new(writer.clone(), size(24), true, Some((0, 0))).unwrap();
    writer.take();
    for (rows, expected, region) in [
        (9, 9, "\x1b[r"),
        (10, 8, "\x1b[1;8r"),
        (40, 38, "\x1b[1;38r"),
    ] {
        assert_eq!(output.resize(size(rows)).unwrap().rows, expected);
        assert!(String::from_utf8(writer.take()).unwrap().contains(region));
    }
    drop(output);
    assert!(String::from_utf8(writer.take()).unwrap().contains("\x1b[r"));
}

#[test]
fn resizes_wait_for_split_sequences_and_synchronized_updates() {
    for (prefix, suffix) in [
        (b"\x1b[31".as_slice(), b"m".as_slice()),
        (b"\x1b]0;title\x1b", b"\\"),
        (b"\x1b_Gimage", b"\x1b\\"),
        (&"猫".as_bytes()[..1], &"猫".as_bytes()[1..]),
        (b"\x1b[?2026hhello", b"\x1b[?2026l"),
    ] {
        let writer = Writer::default();
        let mut output = Output::new(writer.clone(), size(24), true, Some((0, 0))).unwrap();
        writer.take();
        output.write_all(prefix).unwrap();
        assert_eq!(writer.take(), prefix);
        output.resize(size(40)).unwrap();
        assert!(writer.take().is_empty());
        output.write_all(suffix).unwrap();
        let bytes = writer.take();
        assert!(bytes.starts_with(suffix));
        assert!(bytes.windows(7).any(|bytes| bytes == b"[1;38r\x1b"));
    }
}

#[test]
fn disabled_images_forward_bytes_without_terminal_changes() {
    let writer = Writer::default();
    let mut output = Output::new(writer.clone(), size(24), false, None).unwrap();
    assert_eq!(output.resize(size(40)).unwrap(), size(40));
    output.write_all(b"\x1b[31mshell> ").unwrap();
    drop(output);
    assert_eq!(writer.take(), b"\x1b[31mshell> ");
}

#[test]
fn unwinding_restores_the_full_region_even_after_incomplete_output() {
    let writer = Writer::default();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut output = Output::new(writer.clone(), size(24), true, Some((0, 0))).unwrap();
        writer.take();
        output.write_all(b"\x1b_Gunfinished").unwrap();
        panic!("test failure");
    }));
    assert!(result.is_err());
    let bytes = writer.take();
    assert!(bytes.windows(3).any(|bytes| bytes == b"\x1b[r"));
    assert!(bytes.windows(3).any(|bytes| bytes == b"\x18\x1b\\"));
}
