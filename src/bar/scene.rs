//! What is in the bar and where: the cat tree in the corner, the bowl, and the cat.
//!
//! Each thing is shown again on every draw. A new picture goes up before the old one comes
//! down, so there is never a moment with no cat. Two IDs per thing take turns for this.

use super::BAR_ROWS;
use crate::escape;
use crate::frames::{CANVAS_HEIGHT, Facing, Frames, Pose};
use crate::image::{self, IdRange, Placement};
use std::io::{self, Write};

/// When the terminal does not say how big a character cell is, assume it is twice as tall
/// as it is wide, which is close for most fonts.
const DEFAULT_CELL_HEIGHT_PER_WIDTH: u32 = 2;

/// IDs each thing uses, so the new picture can go up before the old one comes down.
const IDS_PER_THING: u32 = 2;

/// Something shown in the bar.
struct Thing {
    pose: Pose,
    /// How far from the left edge, in sprite pixels. Frames are `CANVAS_HEIGHT` pixels tall.
    x: u32,
    /// Higher layers are drawn in front: the bowl is in front of the cat while it eats.
    layer: u32,
}

/// The things in the bar, left to right, with the same spots as the kitty test player.
const THINGS: [Thing; 3] = [
    Thing {
        pose: Pose::Tower,
        x: 0,
        layer: 0,
    },
    Thing {
        pose: Pose::Bowl,
        x: 166,
        layer: 2,
    },
    // The cat sits at its home spot, just past the bowl.
    Thing {
        pose: Pose::Sit,
        x: 222,
        layer: 1,
    },
];

/// The size of one character cell in pixels, when the terminal reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellSize {
    pub width: u32,
    pub height: u32,
}

impl CellSize {
    /// Works out the cell size from the window's size in cells and in pixels.
    pub fn from_window(
        columns: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Option<Self> {
        let width = u32::from(pixel_width).checked_div(u32::from(columns))?;
        let height = u32::from(pixel_height).checked_div(u32::from(rows))?;
        (width > 0 && height > 0).then_some(Self { width, height })
    }

    /// The column, counted from 0, that a spot `x` sprite pixels from the left falls in.
    fn column(cell: Option<Self>, x: u32) -> u16 {
        // A frame is `BAR_ROWS` cells tall, so one sprite pixel is this big on screen.
        let on_screen = match cell {
            Some(cell) => x * u32::from(BAR_ROWS) * cell.height / (CANVAS_HEIGHT * cell.width),
            None => x * u32::from(BAR_ROWS) * DEFAULT_CELL_HEIGHT_PER_WIDTH / CANVAS_HEIGHT,
        };
        u16::try_from(on_screen).unwrap_or(u16::MAX)
    }
}

/// The things in the bar and the pictures showing them now.
pub struct Scene {
    frames: Frames,
    ids: IdRange,
    /// The ID showing each thing, if it is on screen.
    shown: [Option<u32>; THINGS.len()],
}

impl Scene {
    pub fn new(frames: Frames) -> Self {
        Self {
            frames,
            ids: IdRange::pick(),
            shown: [None; THINGS.len()],
        }
    }

    /// Shows every thing in the bar, whose top row is `bar_row` (counted from 1).
    /// The cursor is left moved; the caller puts it back.
    pub fn draw(
        &mut self,
        writer: &mut impl Write,
        bar_row: u16,
        cell: Option<CellSize>,
    ) -> io::Result<()> {
        for (index, thing) in THINGS.iter().enumerate() {
            let slot = index as u32 * IDS_PER_THING;
            // Take the other ID of the pair from the one on screen now.
            let id = match self.shown[index] {
                Some(old) if old == self.ids.id(slot) => self.ids.id(slot + 1),
                _ => self.ids.id(slot),
            };
            let column = CellSize::column(cell, thing.x);
            writer.write_all(escape::move_cursor(bar_row, column + 1).as_bytes())?;
            let png = self
                .frames
                .get(thing.pose, 0, Facing::Right)
                .map_err(io::Error::other)?;
            let placement = Placement {
                rows: BAR_ROWS,
                layer: thing.layer,
            };
            image::show(writer, id, png, placement, false)?;
            if let Some(old) = self.shown[index].replace(id) {
                image::remove(writer, old, false)?;
            }
        }
        Ok(())
    }

    /// Takes every thing off the screen, such as when the bar turns off.
    pub fn clear(&mut self, writer: &mut impl Write) -> io::Result<()> {
        for shown in &mut self.shown {
            if let Some(id) = shown.take() {
                image::remove(writer, id, false)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};

    #[test]
    fn sprite_pixels_turn_into_columns() {
        // Cells 10 px wide and 20 px tall: a frame two rows tall is 40 px high,
        // so the cat's home spot at 222 sprite pixels is about 41 px, or column 4.
        let cell = Some(CellSize {
            width: 10,
            height: 20,
        });
        assert_eq!(CellSize::column(cell, 0), 0);
        assert_eq!(CellSize::column(cell, 222), 4);
        assert_eq!(CellSize::column(None, 222), 4);
    }

    #[test]
    fn unknown_cell_sizes_stay_unknown() {
        assert_eq!(CellSize::from_window(80, 24, 0, 0), None);
        assert_eq!(CellSize::from_window(0, 0, 800, 480), None);
        assert_eq!(
            CellSize::from_window(80, 24, 800, 480),
            Some(CellSize {
                width: 10,
                height: 20
            })
        );
    }

    #[test]
    fn a_new_frame_goes_up_before_the_old_one_comes_down() -> Result<()> {
        let mut scene = Scene::new(Frames::new(None));
        let mut first = Vec::new();
        scene.draw(&mut first, 23, None)?;
        let ids = scene.shown;
        assert!(
            !contains(&first, b"a=d"),
            "nothing to remove the first time"
        );

        let mut second = Vec::new();
        scene.draw(&mut second, 23, None)?;
        for (old, new) in ids.iter().zip(scene.shown) {
            let (Some(old), Some(new)) = (old, new) else {
                anyhow::bail!("every thing should be shown");
            };
            assert_ne!(*old, new);
            let shown_at = find(&second, format!("i={new},").as_bytes()).context("shown")?;
            let removed_at =
                find(&second, format!("a=d,d=I,i={old},").as_bytes()).context("removed")?;
            assert!(shown_at < removed_at, "the new picture goes up first");
        }

        let mut cleared = Vec::new();
        scene.clear(&mut cleared)?;
        assert!(scene.shown.iter().all(Option::is_none));
        assert_eq!(
            cleared.windows(3).filter(|bytes| *bytes == b"a=d").count(),
            THINGS.len()
        );
        Ok(())
    }

    fn contains(bytes: &[u8], part: &[u8]) -> bool {
        find(bytes, part).is_some()
    }

    fn find(bytes: &[u8], part: &[u8]) -> Option<usize> {
        bytes.windows(part.len()).position(|window| window == part)
    }
}
