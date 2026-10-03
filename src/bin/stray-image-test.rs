//! Phase 0: the image test.
//!
//! Runs a placeholder cat along the bottom two rows of the terminal and back,
//! using the kitty image protocol. Uses only the standard library, and `stty`
//! to put the terminal in raw mode.
//!
//! This program is thrown away once Phase 0 is done.

use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const USAGE: &str = "\
Usage: stray-image-test [--resend] [--dark]

Runs a placeholder cat along the bottom two rows and back.
Press q or Ctrl C to stop early.

  --resend  send the whole picture on every step, instead of placing it by ID
  --dark    draw a dark cat, for light backgrounds";

/// Size of one frame in pixels.
const WIDTH: usize = 96;
const HEIGHT: usize = 60;

/// Frames in one running loop. Each also gets a flipped copy.
const FRAME_COUNT: usize = 12;

/// How much of the screen the cat covers.
const CAT_COLUMNS: usize = 6;
const CAT_ROWS: usize = 2;

/// Our image IDs start at an unusual number so they do not clash with
/// images from other programs.
const FIRST_ID: u32 = 7_340_000;

/// The image ID used only by the image check.
const CHECK_ID: u32 = 31;

/// Time between two steps of the run.
const STEP_TIME: Duration = Duration::from_millis(60);

/// How long to wait for the terminal to answer the image check.
const CHECK_TIMEOUT: Duration = Duration::from_secs(1);

/// Each pixel is tested this many times across and down, so edges come out smooth.
const SAMPLES: usize = 4;

const LIGHT_CAT: [u8; 3] = [235, 235, 235];
const DARK_CAT: [u8; 3] = [30, 30, 30];

fn main() {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    if options.help {
        println!("{USAGE}");
        return;
    }

    match run(&options) {
        Ok(Outcome::Ran { placed }) => println!("Placed {placed} frames."),
        Ok(Outcome::NoImages) => {
            println!("This terminal cannot show images, so the cat cannot run here.");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("stray-image-test: {error}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct Options {
    resend: bool,
    dark: bool,
    help: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        for arg in args {
            match arg.as_str() {
                "--resend" => options.resend = true,
                "--dark" => options.dark = true,
                "-h" | "--help" => options.help = true,
                other => return Err(format!("Unknown option: {other}")),
            }
        }
        Ok(options)
    }
}

enum Outcome {
    Ran { placed: usize },
    NoImages,
}

fn run(options: &Options) -> io::Result<Outcome> {
    let (rows, columns) = terminal_size()?;
    if rows <= CAT_ROWS || columns <= CAT_COLUMNS {
        return Err(io::Error::other("the window is too small"));
    }

    // Restores the terminal when it goes out of scope, even on an error.
    let raw = RawMode::start()?;
    let mut out = io::stdout().lock();

    if !check_images(&mut out)? {
        return Ok(Outcome::NoImages);
    }

    // During the run, reads must return at once so a step is never held up.
    raw.wait_for_input(0)?;

    let color = if options.dark { DARK_CAT } else { LIGHT_CAT };
    let frames = make_frames(color);

    if !options.resend {
        for (index, frame) in frames.iter().enumerate() {
            upload(&mut out, frame_id(index), frame, None)?;
        }
    }

    // Save the cursor and hide it while the cat runs.
    out.write_all(b"\x1b7\x1b[?25l")?;
    out.flush()?;

    let result = run_cat(&mut out, &frames, rows, columns, options.resend);

    // Clean up even if the run failed part way.
    for index in 0..frames.len() {
        delete_image(&mut out, frame_id(index))?;
    }
    out.write_all(b"\x1b8\x1b[?25h")?;
    out.flush()?;

    let placed = result?;
    Ok(Outcome::Ran { placed })
}

/// Runs the cat there and back along the bottom rows. Returns how many frames were placed.
fn run_cat(
    out: &mut impl Write,
    frames: &[Vec<u8>],
    rows: usize,
    columns: usize,
    resend: bool,
) -> io::Result<usize> {
    let top_row = rows - CAT_ROWS + 1;
    let mut shown: Option<Shown> = None;
    let mut placed = 0;

    for (step, spot) in path(columns).into_iter().enumerate() {
        if stop_pressed()? {
            break;
        }

        let mut frame = step % FRAME_COUNT;
        if !spot.facing_right {
            frame += FRAME_COUNT;
        }

        // Two placement IDs take turns, so the new frame can be placed
        // before the old one is removed. That way there is never a moment with no cat.
        let placement = 1 + (step % 2) as u32;

        write!(out, "\x1b[{top_row};{}H", spot.column)?;
        let new = if resend {
            // Two image slots also take turns, for the same reason.
            let id = FIRST_ID + 100 + (step % 2) as u32;
            upload(out, id, &frames[frame], Some(placement))?;
            Shown { id, placement }
        } else {
            let id = frame_id(frame);
            place(out, id, placement)?;
            Shown { id, placement }
        };

        if let Some(old) = shown {
            remove(out, old, resend)?;
        }
        shown = Some(new);
        placed += 1;

        out.flush()?;
        thread::sleep(STEP_TIME);
    }

    if let Some(old) = shown {
        remove(out, old, resend)?;
        out.flush()?;
    }
    Ok(placed)
}

#[derive(Debug, PartialEq)]
struct Spot {
    /// Column of the cat's left edge, counting from 1.
    column: usize,
    facing_right: bool,
}

/// The spots the cat visits: left to right, then back again.
fn path(columns: usize) -> Vec<Spot> {
    let last = columns - CAT_COLUMNS + 1;
    let there = (1..=last).map(|column| Spot {
        column,
        facing_right: true,
    });
    let back = (1..last).rev().map(|column| Spot {
        column,
        facing_right: false,
    });
    there.chain(back).collect()
}

/// What is on screen right now, so it can be removed.
#[derive(Clone, Copy)]
struct Shown {
    id: u32,
    placement: u32,
}

fn frame_id(index: usize) -> u32 {
    FIRST_ID + index as u32
}

// The image protocol.
//
// Every command ends with q=2, which tells the terminal not to answer.
// An answer would arrive as if it were typed.

/// Sends a picture to the terminal. With a placement, it is also shown at the cursor.
fn upload(out: &mut impl Write, id: u32, rgba: &[u8], placement: Option<u32>) -> io::Result<()> {
    let data = base64(rgba);
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();

    for (index, chunk) in chunks.iter().enumerate() {
        let more = if index + 1 < chunks.len() { 1 } else { 0 };
        if index == 0 {
            let action = match placement {
                Some(p) => format!("a=T,p={p},c={CAT_COLUMNS},r={CAT_ROWS},C=1"),
                None => "a=t".to_string(),
            };
            write!(
                out,
                "\x1b_G{action},f=32,s={WIDTH},v={HEIGHT},i={id},q=2,m={more};"
            )?;
        } else {
            write!(out, "\x1b_Gm={more};")?;
        }
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
    }
    Ok(())
}

/// Shows an uploaded picture at the cursor. C=1 keeps the cursor where it is.
fn place(out: &mut impl Write, id: u32, placement: u32) -> io::Result<()> {
    write!(
        out,
        "\x1b_Ga=p,i={id},p={placement},c={CAT_COLUMNS},r={CAT_ROWS},C=1,q=2\x1b\\"
    )
}

/// Takes a frame off the screen. In resend mode the picture itself is freed too.
fn remove(out: &mut impl Write, shown: Shown, resend: bool) -> io::Result<()> {
    if resend {
        delete_image(out, shown.id)
    } else {
        let Shown { id, placement } = shown;
        write!(out, "\x1b_Ga=d,d=i,i={id},p={placement},q=2\x1b\\")
    }
}

/// Removes a picture and everything showing it.
fn delete_image(out: &mut impl Write, id: u32) -> io::Result<()> {
    write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

// The image check.

/// Asks the terminal whether it can show images.
///
/// Sends a tiny picture marked "check only", then a standard question every
/// terminal answers. An OK for the picture before that answer means yes.
fn check_images(out: &mut impl Write) -> io::Result<bool> {
    write!(
        out,
        "\x1b_Gi={CHECK_ID},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c"
    )?;
    out.flush()?;

    let mut reply = Vec::new();
    let mut buffer = [0u8; 256];
    let started = Instant::now();
    let mut stdin = io::stdin().lock();

    while started.elapsed() < CHECK_TIMEOUT && !has_standard_answer(&reply) {
        // Raw mode makes this return after a tenth of a second if nothing arrives.
        let count = stdin.read(&mut buffer)?;
        reply.extend_from_slice(&buffer[..count]);
    }
    Ok(says_yes(&reply))
}

/// The standard answer looks like ESC [ ? 6 2 ; 2 2 c
fn has_standard_answer(reply: &[u8]) -> bool {
    match find(reply, b"\x1b[?") {
        Some(start) => reply[start..].contains(&b'c'),
        None => false,
    }
}

fn says_yes(reply: &[u8]) -> bool {
    let ok = find(reply, format!("\x1b_Gi={CHECK_ID};OK").as_bytes());
    let standard = find(reply, b"\x1b[?");
    match (ok, standard) {
        (Some(ok), Some(standard)) => ok < standard,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// True if q or Ctrl C was pressed.
fn stop_pressed() -> io::Result<bool> {
    let mut buffer = [0u8; 64];
    let count = io::stdin().lock().read(&mut buffer)?;
    Ok(buffer[..count].iter().any(|&key| key == b'q' || key == 3))
}

// The terminal.

/// Raw mode: keys arrive one at a time, are not shown, and Ctrl C does not stop us.
/// The old settings come back when this is dropped.
struct RawMode {
    saved: String,
}

impl RawMode {
    fn start() -> io::Result<RawMode> {
        let saved = stty(&["-g"])?;
        stty(&["-icanon", "-echo", "-isig", "min", "0", "time", "1"])?;
        Ok(RawMode {
            saved: saved.trim().to_string(),
        })
    }

    /// How long a read waits for input, in tenths of a second. 0 returns at once.
    fn wait_for_input(&self, tenths: u8) -> io::Result<()> {
        stty(&["time", &tenths.to_string()]).map(|_| ())
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = stty(&[&self.saved]);
    }
}

/// Returns the window size as rows and columns.
fn terminal_size() -> io::Result<(usize, usize)> {
    let size = stty(&["size"])?;
    let mut numbers = size.split_whitespace().map(str::parse::<usize>);
    match (numbers.next(), numbers.next()) {
        (Some(Ok(rows)), Some(Ok(columns))) => Ok((rows, columns)),
        _ => Err(io::Error::other(format!(
            "could not read the window size from {size:?}"
        ))),
    }
}

/// Runs stty on our terminal and returns what it printed.
fn stty(args: &[&str]) -> io::Result<String> {
    let output = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("this must run in a terminal window"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// Drawing the placeholder cat.

/// All frames facing right, then the same frames flipped to face left.
fn make_frames(color: [u8; 3]) -> Vec<Vec<u8>> {
    let right: Vec<Vec<u8>> = (0..FRAME_COUNT)
        .map(|index| draw_cat(index as f32 / FRAME_COUNT as f32, color))
        .collect();
    let left: Vec<Vec<u8>> = right.iter().map(|frame| flip(frame)).collect();
    right.into_iter().chain(left).collect()
}

/// Draws one frame of the run. `phase` goes from 0 to 1 over one stride.
/// Returns RGBA pixels. Only the alpha changes from pixel to pixel.
fn draw_cat(phase: f32, color: [u8; 3]) -> Vec<u8> {
    let cat = Cat::at(phase);
    let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let mut hits = 0;
            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let px = x as f32 + (sample_x as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sample_y as f32 + 0.5) / SAMPLES as f32;
                    if cat.covers((px, py)) {
                        hits += 1;
                    }
                }
            }
            let index = (y * WIDTH + x) * 4;
            rgba[index..index + 3].copy_from_slice(&color);
            rgba[index + 3] = (hits * 255 / (SAMPLES * SAMPLES)) as u8;
        }
    }
    rgba
}

/// Mirrors a frame left to right.
fn flip(rgba: &[u8]) -> Vec<u8> {
    let mut flipped = Vec::with_capacity(rgba.len());
    for row in rgba.chunks(WIDTH * 4) {
        for pixel in row.chunks(4).rev() {
            flipped.extend_from_slice(pixel);
        }
    }
    flipped
}

type Point = (f32, f32);

/// The cat's shape at one moment, facing right. Made of simple shapes.
struct Cat {
    /// How far the body rises above its resting height.
    lift: f32,
    front_feet: [Point; 2],
    back_feet: [Point; 2],
    tail_tip: Point,
}

const LEG_LENGTH: f32 = 18.0;
const LEG_RADIUS: f32 = 2.5;
const SHOULDER: Point = (62.0, 36.0);
const HIP: Point = (30.0, 36.0);

impl Cat {
    fn at(phase: f32) -> Cat {
        let turn = phase * std::f32::consts::TAU;
        let lift = 2.0 * (turn * 2.0).sin();

        // Front and back legs swing in opposite directions.
        let foot = |joint: Point, angle: f32| {
            (
                joint.0 + LEG_LENGTH * angle.sin(),
                joint.1 - lift + LEG_LENGTH * angle.cos(),
            )
        };
        let swing = 0.7;
        Cat {
            lift,
            front_feet: [
                foot(SHOULDER, swing * turn.sin()),
                foot(SHOULDER, swing * (turn + 0.6).sin()),
            ],
            back_feet: [
                foot(HIP, -swing * turn.sin()),
                foot(HIP, -swing * (turn + 0.6).sin()),
            ],
            tail_tip: (8.0, 18.0 - lift + 5.0 * turn.sin()),
        }
    }

    fn covers(&self, p: Point) -> bool {
        let up = |(x, y): Point| (x, y - self.lift);
        let shoulder = up(SHOULDER);
        let hip = up(HIP);

        in_ellipse(p, up((46.0, 33.0)), 25.0, 10.0)
            || in_circle(p, up((74.0, 25.0)), 9.0)
            || in_triangle(p, up((67.0, 19.0)), up((70.0, 8.0)), up((75.0, 17.0)))
            || in_triangle(p, up((75.0, 17.0)), up((81.0, 8.0)), up((83.0, 21.0)))
            || near_segment(p, up((24.0, 30.0)), self.tail_tip, LEG_RADIUS)
            || self
                .front_feet
                .iter()
                .any(|&f| near_segment(p, shoulder, f, LEG_RADIUS))
            || self
                .back_feet
                .iter()
                .any(|&f| near_segment(p, hip, f, LEG_RADIUS))
    }
}

fn in_ellipse(p: Point, center: Point, radius_x: f32, radius_y: f32) -> bool {
    let dx = (p.0 - center.0) / radius_x;
    let dy = (p.1 - center.1) / radius_y;
    dx * dx + dy * dy <= 1.0
}

fn in_circle(p: Point, center: Point, radius: f32) -> bool {
    in_ellipse(p, center, radius, radius)
}

fn in_triangle(p: Point, a: Point, b: Point, c: Point) -> bool {
    let side = |from: Point, to: Point| {
        (to.0 - from.0) * (p.1 - from.1) - (to.1 - from.1) * (p.0 - from.0)
    };
    let (ab, bc, ca) = (side(a, b), side(b, c), side(c, a));
    (ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0)
}

fn near_segment(p: Point, a: Point, b: Point, radius: f32) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    cx * cx + cy * cy <= radius * radius
}

// Base64, which the image protocol uses to send pictures as text.

fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for group in bytes.chunks(3) {
        let b = [
            group[0],
            *group.get(1).unwrap_or(&0),
            *group.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= group.len() {
                text.push(LETTERS[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0, 0, 0]), "AAAA");
    }

    #[test]
    fn image_check_reads_the_reply() {
        let yes = format!("\x1b_Gi={CHECK_ID};OK\x1b\\\x1b[?62;22c");
        assert!(says_yes(yes.as_bytes()));
        assert!(has_standard_answer(yes.as_bytes()));

        let no = b"\x1b[?62;22c";
        assert!(!says_yes(no));
        assert!(has_standard_answer(no));

        assert!(!says_yes(b""));
        assert!(!has_standard_answer(b"\x1b[?62;2"));
    }

    #[test]
    fn path_goes_there_and_back() {
        let spots = path(10);
        // Columns 1 to 5 going right, then 4 down to 1 going left.
        assert_eq!(spots.len(), 9);
        assert_eq!(
            spots[0],
            Spot {
                column: 1,
                facing_right: true
            }
        );
        assert_eq!(
            spots[4],
            Spot {
                column: 5,
                facing_right: true
            }
        );
        assert_eq!(
            spots[5],
            Spot {
                column: 4,
                facing_right: false
            }
        );
        assert_eq!(
            spots[8],
            Spot {
                column: 1,
                facing_right: false
            }
        );
    }

    #[test]
    fn frames_have_a_cat_with_smooth_edges() {
        let frames = make_frames(LIGHT_CAT);
        assert_eq!(frames.len(), FRAME_COUNT * 2);

        for frame in &frames {
            assert_eq!(frame.len(), WIDTH * HEIGHT * 4);
            let alphas: Vec<u8> = frame.chunks(4).map(|pixel| pixel[3]).collect();
            assert!(alphas.contains(&255), "the cat has a solid middle");
            assert!(alphas.contains(&0), "the cat has empty space around it");
            assert!(
                alphas.iter().any(|&a| a > 0 && a < 255),
                "the edges are smooth"
            );
        }
    }

    #[test]
    fn flipping_twice_gives_the_same_frame() {
        let frame = draw_cat(0.3, LIGHT_CAT);
        assert_ne!(flip(&frame), frame);
        assert_eq!(flip(&flip(&frame)), frame);
    }

    #[test]
    fn options_are_read() {
        let args = ["--dark", "--resend"].map(String::from).into_iter();
        let options = Options::parse(args).unwrap();
        assert_eq!(
            options,
            Options {
                resend: true,
                dark: true,
                help: false
            }
        );
        assert!(Options::parse(["--fast".to_string()].into_iter()).is_err());
    }
}
