use portable_pty::PtySize;
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;
use stray_cat::bar::Output;
use stray_cat::escape::{RESET_SCROLL_REGION, RESTORE_TERMINAL};

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

/// A terminal stand-in. Clones share one buffer, so the test can read what `Output` wrote.
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
    /// Returns everything written so far and empties the buffer.
    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

fn contains(bytes: &[u8], part: &[u8]) -> bool {
    bytes.windows(part.len()).any(|window| window == part)
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
fn scrolling_leaves_the_bar_empty_and_preserves_the_programs_saved_cursor() -> Result {
    let writer = Writer::default();
    let mut terminal = vt100::Parser::new(24, 80, 0);
    terminal.process(b"\x1b[6;7H\x1b7\x1b[8;9H");
    let mut output = Output::new(writer.clone(), size(24), true, Some((7, 8)))?;
    terminal.process(&writer.take());
    assert_eq!(terminal.screen().cursor_position(), (7, 8));

    output.write_all(b"\x1b[22;1Hone\r\ntwo\r\nthree")?;
    terminal.process(&writer.take());
    let rows: Vec<_> = terminal.screen().rows(0, 80).collect();
    assert_eq!(&rows[19..], ["one", "two", "three", "", ""]);

    output.resize(size(24))?;
    terminal.process(&writer.take());
    terminal.process(b"\x1b8");
    assert_eq!(terminal.screen().cursor_position(), (5, 6));
    Ok(())
}

#[test]
fn resizing_across_the_height_boundary_updates_the_region() -> Result {
    let writer = Writer::default();
    let mut output = Output::new(writer.clone(), size(24), true, Some((0, 0)))?;
    writer.take();
    for (rows, expected, region) in [
        (9, 9, "\x1b[r"),
        (10, 8, "\x1b[1;8r"),
        (40, 38, "\x1b[1;38r"),
    ] {
        assert_eq!(output.resize(size(rows))?.rows, expected);
        assert!(contains(&writer.take(), region.as_bytes()));
    }
    drop(output);
    assert!(contains(&writer.take(), RESET_SCROLL_REGION));
    Ok(())
}

#[test]
fn resizes_wait_for_split_sequences_and_synchronized_updates() -> Result {
    for (prefix, suffix) in [
        (b"\x1b[31".as_slice(), b"m".as_slice()),
        (b"\x1b]0;title\x1b", b"\\"),
        (b"\x1b_Gimage", b"\x1b\\"),
        (&"猫".as_bytes()[..1], &"猫".as_bytes()[1..]),
        (b"\x1b[?2026hhello", b"\x1b[?2026l"),
    ] {
        let writer = Writer::default();
        let mut output = Output::new(writer.clone(), size(24), true, Some((0, 0)))?;
        writer.take();
        output.write_all(prefix)?;
        assert_eq!(writer.take(), prefix);
        output.resize(size(40))?;
        assert!(writer.take().is_empty());
        output.write_all(suffix)?;
        let bytes = writer.take();
        assert!(bytes.starts_with(suffix));
        assert!(contains(&bytes, b"[1;38r\x1b"));
    }
    Ok(())
}

#[test]
fn disabled_images_forward_bytes_without_terminal_changes() -> Result {
    let writer = Writer::default();
    let mut output = Output::new(writer.clone(), size(24), false, None)?;
    assert_eq!(output.resize(size(40))?, size(40));
    output.write_all(b"\x1b[31mshell> ")?;
    drop(output);
    assert_eq!(writer.take(), b"\x1b[31mshell> ");
    Ok(())
}

#[test]
fn unwinding_restores_the_full_region_even_after_incomplete_output() -> Result {
    let writer = Writer::default();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> io::Result<()> {
        let mut output = Output::new(writer.clone(), size(24), true, Some((0, 0)))?;
        writer.take();
        output.write_all(b"\x1b_Gunfinished")?;
        panic!("test failure");
    }));
    assert!(result.is_err());
    let bytes = writer.take();
    assert!(contains(&bytes, &RESTORE_TERMINAL.concat()));
    Ok(())
}
