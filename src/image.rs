//! The kitty image protocol: show a frame at the cursor, and remove it again.
//!
//! Every command ends with `q=2`, which tells the terminal not to answer. An answer would
//! arrive as if it were typed. A frame is never placed again by its ID: in VS Code,
//! removing a frame from the screen also deletes the picture, so each change sends it again.

use crate::escape::{ESC, GRAPHICS_START, STRING_END, TMUX_PASSTHROUGH_START};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

/// The terminal takes a picture in pieces of at most this many bytes of base64 text.
const CHUNK_SIZE: usize = 4096;

/// IDs below this are left for other programs, which tend to count up from 1.
const FIRST_ID_FLOOR: u32 = 1_000_000;

/// How many IDs each window keeps for itself.
pub const IDS_PER_WINDOW: u32 = 16;

/// How many ID ranges there are to pick from. Kept small enough that every ID fits in a u32.
const ID_RANGES: u32 = 100_000_000;

/// Where a picture goes, measured from the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// How many rows tall the picture is. The terminal works out its width.
    pub rows: u16,
    /// Pictures on a higher layer are drawn in front, like the bowl in front of the cat.
    pub layer: u32,
    /// How many pixels right of the cursor's cell the picture starts, so it can move
    /// smoothly between columns.
    pub x_offset: u32,
}

/// One window's own range of image IDs, so it only ever removes its own pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdRange {
    first: u32,
}

impl IdRange {
    /// Picks a range from the process ID and the time, so two windows almost never share one.
    pub fn pick() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.subsec_nanos());
        Self::from_seed(std::process::id() ^ nanos)
    }

    fn from_seed(seed: u32) -> Self {
        Self {
            first: FIRST_ID_FLOOR + (seed % ID_RANGES) * IDS_PER_WINDOW,
        }
    }

    /// The ID in `slot`, from 0 up to `IDS_PER_WINDOW`. Later slots wrap around.
    pub fn id(self, slot: u32) -> u32 {
        self.first + slot % IDS_PER_WINDOW
    }
}

/// Sends a PNG and shows it at the cursor. The cursor stays where it is (`C=1`).
pub fn show(
    output: &mut impl Write,
    id: u32,
    png: &[u8],
    placement: Placement,
    inside_tmux: bool,
) -> io::Result<()> {
    let data = BASE64.encode(png);
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(CHUNK_SIZE).collect();
    let last = chunks.len().saturating_sub(1);
    for (index, chunk) in chunks.into_iter().enumerate() {
        let more = u8::from(index < last);
        let keys = if index == 0 {
            let Placement {
                rows,
                layer,
                x_offset,
            } = placement;
            // Most pictures start right at the cell, so the offset is left out then.
            let offset = if x_offset > 0 {
                format!(",X={x_offset}")
            } else {
                String::new()
            };
            format!("a=T,f=100,i={id},r={rows},z={layer}{offset},C=1,q=2,m={more}")
        } else {
            format!("m={more}")
        };
        let command = [GRAPHICS_START, keys.as_bytes(), b";", chunk, STRING_END].concat();
        write_command(output, &command, inside_tmux)?;
    }
    Ok(())
}

/// Removes a picture and frees it (`d=I`). Other programs' pictures are left alone.
pub fn remove(output: &mut impl Write, id: u32, inside_tmux: bool) -> io::Result<()> {
    let keys = format!("a=d,d=I,i={id},q=2");
    let command = [GRAPHICS_START, keys.as_bytes(), STRING_END].concat();
    write_command(output, &command, inside_tmux)
}

fn write_command(output: &mut impl Write, command: &[u8], inside_tmux: bool) -> io::Result<()> {
    if inside_tmux {
        output.write_all(&tmux_passthrough(command))
    } else {
        output.write_all(command)
    }
}

/// Wraps bytes so tmux passes them on to the outer terminal. tmux needs every ESC inside
/// written twice.
pub fn tmux_passthrough(bytes: &[u8]) -> Vec<u8> {
    let mut wrapped = Vec::with_capacity(bytes.len() * 2 + TMUX_PASSTHROUGH_START.len());
    wrapped.extend_from_slice(TMUX_PASSTHROUGH_START);
    for &byte in bytes {
        if byte == ESC {
            wrapped.push(ESC);
        }
        wrapped.push(byte);
    }
    wrapped.extend_from_slice(STRING_END);
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};

    const PLACEMENT: Placement = Placement {
        rows: 2,
        layer: 1,
        x_offset: 0,
    };

    /// Splits written output into the key list and the data of each graphics command.
    fn commands(written: &[u8]) -> Result<Vec<(&str, &str)>> {
        let text = std::str::from_utf8(written)?;
        let start = std::str::from_utf8(GRAPHICS_START)?;
        let end = std::str::from_utf8(STRING_END)?;
        let mut found = Vec::new();
        for part in text.split(end).filter(|part| !part.is_empty()) {
            let body = part
                .strip_prefix(start)
                .context("each command starts the same way")?;
            let (keys, data) = body.split_once(';').unwrap_or((body, ""));
            found.push((keys, data));
        }
        Ok(found)
    }

    #[test]
    fn a_small_picture_goes_in_one_command() -> Result<()> {
        let mut written = Vec::new();
        show(&mut written, 7, b"png", PLACEMENT, false)?;
        let found = commands(&written)?;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "a=T,f=100,i=7,r=2,z=1,C=1,q=2,m=0");
        assert_eq!(BASE64.decode(found[0].1)?, b"png");
        Ok(())
    }

    #[test]
    fn an_offset_moves_the_picture_inside_its_cell() -> Result<()> {
        let mut written = Vec::new();
        let placement = Placement {
            x_offset: 4,
            ..PLACEMENT
        };
        show(&mut written, 7, b"png", placement, false)?;
        assert_eq!(
            commands(&written)?[0].0,
            "a=T,f=100,i=7,r=2,z=1,X=4,C=1,q=2,m=0"
        );
        Ok(())
    }

    #[test]
    fn a_big_picture_goes_in_pieces_that_join_back_up() -> Result<()> {
        let png: Vec<u8> = (0..10_000).map(|byte| (byte % 251) as u8).collect();
        let mut written = Vec::new();
        show(&mut written, 7, &png, PLACEMENT, false)?;
        let found = commands(&written)?;
        assert!(found.len() > 1);
        let keys: Vec<&str> = found.iter().map(|(keys, _)| *keys).collect();
        assert!(keys[0].ends_with("m=1"));
        assert!(keys[1..keys.len() - 1].iter().all(|keys| *keys == "m=1"));
        assert_eq!(keys[keys.len() - 1], "m=0");
        let data: String = found.into_iter().map(|(_, data)| data).collect();
        assert_eq!(BASE64.decode(data)?, png);
        Ok(())
    }

    #[test]
    fn removing_frees_only_our_picture() -> Result<()> {
        let mut written = Vec::new();
        remove(&mut written, 7, false)?;
        assert_eq!(written, b"\x1b_Ga=d,d=I,i=7,q=2\x1b\\");
        Ok(())
    }

    #[test]
    fn inside_tmux_every_command_is_passed_through() -> Result<()> {
        let mut written = Vec::new();
        remove(&mut written, 7, true)?;
        assert_eq!(
            written,
            b"\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=7,q=2\x1b\x1b\\\x1b\\"
        );
        Ok(())
    }

    #[test]
    fn id_ranges_stay_apart_and_wrap() {
        let (first, second) = (IdRange::from_seed(1), IdRange::from_seed(2));
        assert_eq!(first.id(0), FIRST_ID_FLOOR + IDS_PER_WINDOW);
        assert_eq!(second.id(0), first.id(IDS_PER_WINDOW - 1) + 1);
        assert_eq!(first.id(IDS_PER_WINDOW), first.id(0));
        let highest = IdRange::from_seed(u32::MAX).id(IDS_PER_WINDOW - 1);
        assert!(highest > FIRST_ID_FLOOR);
    }
}
