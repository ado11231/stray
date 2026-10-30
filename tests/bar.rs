//! Tests the bar's output on its own, with a fake terminal that records every byte.

use portable_pty::PtySize;
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;
use std::time::{Duration, Instant};
use stray_cat::bar::{Output, Setup};
use stray_cat::escape::{self, RESET_SCROLL_REGION, RESTORE_TERMINAL};
use stray_cat::frames::Palette;

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

/// A terminal that shows images, outside tmux, with the cursor at the top left.
const BAR: Setup = Setup {
    images_supported: true,
    inside_tmux: false,
    cursor: Some((0, 0)),
    palette: None,
};

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
    // The default size is 24 rows and 80 columns, the same as `size(24)`.
    let mut terminal = vt100::Parser::default();
    terminal.process(b"\x1b[6;7H\x1b7\x1b[8;9H");
    let mut output = Output::new(
        writer.clone(),
        size(24),
        Setup {
            cursor: Some((7, 8)),
            ..BAR
        },
    )?;
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
    let mut output = Output::new(writer.clone(), size(24), BAR)?;
    writer.take();
    for (rows, shell_rows) in [(9, 9), (10, 8), (40, 38)] {
        assert_eq!(output.resize(size(rows))?.rows, shell_rows);
        let written = writer.take();
        if shell_rows < rows {
            assert!(contains(
                &written,
                escape::scroll_region(shell_rows).as_bytes()
            ));
        } else {
            assert!(contains(&written, RESET_SCROLL_REGION));
        }
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
        let mut output = Output::new(writer.clone(), size(24), BAR)?;
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
    let mut output = Output::new(writer.clone(), size(24), Setup::default())?;
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
        let mut output = Output::new(writer.clone(), size(24), BAR)?;
        writer.take();
        output.write_all(b"\x1b_Gunfinished")?;
        panic!("test failure");
    }));
    assert!(result.is_err());
    let bytes = writer.take();
    assert!(contains(&bytes, &RESTORE_TERMINAL.concat()));
    Ok(())
}

/// The rows the shell gets in a 24 row window, with the bar below them.
const SHELL_ROWS: u16 = 22;

/// How many pictures the bar shows: the cat tree, the bowl, and the cat.
const PICTURES_IN_BAR: usize = 3;

/// The start of a command that shows a picture, and of one that removes it.
const SHOW: &[u8] = b"\x1b_Ga=T";
const REMOVE: &[u8] = b"\x1b_Ga=d,d=I";

fn count(bytes: &[u8], part: &[u8]) -> usize {
    bytes
        .windows(part.len())
        .filter(|window| *window == part)
        .count()
}

#[test]
fn the_cat_and_its_things_are_drawn_in_the_bar_and_removed_on_exit() -> Result {
    let writer = Writer::default();
    let output = Output::new(writer.clone(), size(24), BAR)?;
    let bytes = writer.take();
    assert_eq!(count(&bytes, SHOW), PICTURES_IN_BAR);
    // The cat tree starts the first bar row, just below the 22 shell rows.
    let first_bar_row = escape::move_cursor(SHELL_ROWS + 1, 1);
    assert!(contains(&bytes, &[first_bar_row.as_bytes(), SHOW].concat()));
    drop(output);
    assert_eq!(count(&writer.take(), REMOVE), PICTURES_IN_BAR);
    Ok(())
}

#[test]
fn a_short_window_takes_the_cat_away_until_it_grows() -> Result {
    let writer = Writer::default();
    let mut output = Output::new(writer.clone(), size(24), BAR)?;
    writer.take();
    output.resize(size(9))?;
    assert_eq!(count(&writer.take(), REMOVE), PICTURES_IN_BAR);
    output.resize(size(24))?;
    let bytes = writer.take();
    assert_eq!(count(&bytes, SHOW), PICTURES_IN_BAR);
    assert_eq!(count(&bytes, REMOVE), 0);
    Ok(())
}

#[test]
fn inside_tmux_the_rows_are_kept_but_nothing_is_drawn_yet() -> Result {
    let writer = Writer::default();
    let output = Output::new(
        writer.clone(),
        size(24),
        Setup {
            inside_tmux: true,
            ..BAR
        },
    )?;
    let bytes = writer.take();
    assert!(contains(
        &bytes,
        escape::scroll_region(SHELL_ROWS).as_bytes()
    ));
    assert_eq!(count(&bytes, SHOW), 0);
    drop(output);
    assert_eq!(count(&writer.take(), REMOVE), 0);
    Ok(())
}

#[test]
fn clears_resets_and_full_screen_switches_draw_the_bar_again() -> Result {
    for bytes in [
        b"\x1b[H\x1b[2J".as_slice(),
        b"\x1b[3J",
        b"\x1bc",
        b"\x1b[!p",
        b"\x1b[r",
        b"\x1b[1;24r",
        b"\x1b[?1049h",
        b"\x1b[?1049l",
    ] {
        let writer = Writer::default();
        let mut output = Output::new(writer.clone(), size(24), BAR)?;
        writer.take();
        output.write_all(bytes)?;
        let written = writer.take();
        assert!(
            contains(&written, escape::scroll_region(SHELL_ROWS).as_bytes()),
            "{bytes:?}"
        );
        assert_eq!(count(&written, SHOW), PICTURES_IN_BAR, "{bytes:?}");
    }
    Ok(())
}

#[test]
fn everyday_output_leaves_the_bar_alone() -> Result {
    let writer = Writer::default();
    let mut output = Output::new(writer.clone(), size(24), BAR)?;
    writer.take();
    // Text, clearing below the prompt, a region inside the shell rows, and a move to the
    // last shell row.
    output.write_all(b"hello\r\n\x1b[J\x1b[5;10r\x1b[r\x1b[1;22r\x1b[22;1H")?;
    let written = writer.take();
    assert_eq!(count(&written, SHOW), 0);
    assert_eq!(
        count(&written, escape::scroll_region(SHELL_ROWS).as_bytes()),
        1,
        "only the program's own region"
    );
    Ok(())
}

#[test]
fn a_redraw_waits_for_a_synchronized_update_to_end() -> Result {
    let writer = Writer::default();
    let mut output = Output::new(writer.clone(), size(24), BAR)?;
    writer.take();
    output.write_all(b"\x1b[?2026h\x1b[2J")?;
    assert_eq!(count(&writer.take(), SHOW), 0);
    output.write_all(b"\x1b[?2026l")?;
    assert_eq!(count(&writer.take(), SHOW), PICTURES_IN_BAR);
    Ok(())
}

#[test]
fn cursor_moves_into_the_bar_are_sent_back_to_the_last_shell_row() -> Result {
    let writer = Writer::default();
    let mut terminal = vt100::Parser::default();
    let mut output = Output::new(writer.clone(), size(24), BAR)?;
    terminal.process(&writer.take());
    for (bytes, expected) in [
        (b"\x1b[24;5H".as_slice(), (21, 4)),
        (b"\x1b[3;1H\x1b[30d", (21, 0)),
        (b"\x1b[999;999H", (21, 79)),
    ] {
        output.write_all(bytes)?;
        terminal.process(&writer.take());
        assert_eq!(terminal.screen().cursor_position(), expected, "{bytes:?}");
    }
    Ok(())
}

#[test]
fn the_terminal_colors_reach_the_pictures() -> Result {
    let plain = Writer::default();
    drop(Output::new(plain.clone(), size(24), BAR)?);
    let colored = Writer::default();
    let palette = Palette {
        text: [200, 180, 160],
        background: [10, 20, 30],
    };
    let setup = Setup {
        palette: Some(palette),
        ..BAR
    };
    drop(Output::new(colored.clone(), size(24), setup)?);
    let (plain, colored) = (plain.take(), colored.take());
    assert_eq!(count(&colored, SHOW), PICTURES_IN_BAR);
    assert_ne!(plain, colored);
    Ok(())
}

#[test]
fn the_cat_moves_on_and_the_cursor_goes_back() -> Result {
    let writer = Writer::default();
    let mut terminal = vt100::Parser::default();
    let mut output = Output::new(writer.clone(), size(24), BAR)?;
    output.write_all(b"\x1b[5;6Hprompt")?;
    terminal.process(&writer.take());
    let cursor = terminal.screen().cursor_position();

    // A second later the cat shows a later frame of its sit.
    let later = Instant::now() + Duration::from_secs(1);
    let next = output.animate(later)?.ok_or("the cat should keep moving")?;
    let drawn = writer.take();
    terminal.process(&drawn);
    assert!(next > later);
    assert_eq!(drawn.windows(3).filter(|bytes| *bytes == b"a=T").count(), 1);
    assert_eq!(terminal.screen().cursor_position(), cursor);

    // Asked again for the same moment, nothing changed, so nothing is sent.
    output.animate(later)?;
    assert!(writer.take().is_empty());
    Ok(())
}

#[test]
fn the_cat_waits_while_a_sequence_is_open() -> Result {
    let writer = Writer::default();
    let mut output = Output::new(writer.clone(), size(24), BAR)?;
    output.write_all(b"\x1b[")?;
    writer.take();

    let later = Instant::now() + Duration::from_secs(1);
    let retry = output.animate(later)?.ok_or("the cat should try again")?;
    assert!(
        writer.take().is_empty(),
        "nothing may cut into the sequence"
    );
    assert!(retry > later);

    output.write_all(b"m")?;
    writer.take();
    output.animate(later)?;
    assert!(contains(&writer.take(), b"a=T"));
    Ok(())
}

#[test]
fn without_images_there_is_no_cat_to_move() -> Result {
    let writer = Writer::default();
    let setup = Setup {
        images_supported: false,
        ..BAR
    };
    let mut output = Output::new(writer.clone(), size(24), setup)?;
    writer.take();

    assert_eq!(output.animate(Instant::now())?, None);
    assert!(writer.take().is_empty());
    Ok(())
}
