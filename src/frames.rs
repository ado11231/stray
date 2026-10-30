//! The cat's frames and the things in its bar, built into the program from `assets/sprites`.
//!
//! A frame is colored to match the terminal and flipped to face left the first time it is
//! needed, then kept. So starting Stray never waits for every frame to be ready.

mod color;
mod pixels;

pub use color::Palette;

use anyhow::{Context, Result};
use std::borrow::Cow;
use std::collections::HashMap;

/// Every sprite folder and its frames, in order. `build.rs` writes this table.
const SPRITES: &[(&str, &[&[u8]])] = include!(concat!(env!("OUT_DIR"), "/sprites.rs"));

/// Red, green, blue, and how see-through.
const BYTES_PER_PIXEL: usize = 4;

/// Every frame is this many pixels tall, with the feet on the same row, so a spot in
/// sprite pixels can be turned into a spot on screen.
pub const CANVAS_HEIGHT: u32 = 215;

/// A pose of the cat, or a thing in its bar. Each is named after its folder in
/// `assets/sprites`. To add one, add it here, to `ALL`, and to `folder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pose {
    Sleep,
    Wake,
    Stand,
    Sit,
    SitDown,
    Walk,
    Run,
    Turn,
    Lean,
    Eat,
    Crouch,
    Wiggle,
    Pounce,
    Bat,
    LookUp,
    Flinch,
    PerkUp,
    PettedIn,
    Purr,
    Bowl,
    Ball,
    Tower,
}

impl Pose {
    pub const ALL: [Pose; 22] = [
        Pose::Sleep,
        Pose::Wake,
        Pose::Stand,
        Pose::Sit,
        Pose::SitDown,
        Pose::Walk,
        Pose::Run,
        Pose::Turn,
        Pose::Lean,
        Pose::Eat,
        Pose::Crouch,
        Pose::Wiggle,
        Pose::Pounce,
        Pose::Bat,
        Pose::LookUp,
        Pose::Flinch,
        Pose::PerkUp,
        Pose::PettedIn,
        Pose::Purr,
        Pose::Bowl,
        Pose::Ball,
        Pose::Tower,
    ];

    /// The folder in `assets/sprites` its frames come from.
    pub fn folder(self) -> &'static str {
        match self {
            Pose::Sleep => "sleep",
            Pose::Wake => "wake",
            Pose::Stand => "stand",
            Pose::Sit => "sit",
            Pose::SitDown => "sit_down",
            Pose::Walk => "walk",
            Pose::Run => "run",
            Pose::Turn => "turn",
            Pose::Lean => "lean",
            Pose::Eat => "eat",
            Pose::Crouch => "crouch",
            Pose::Wiggle => "wiggle",
            Pose::Pounce => "pounce",
            Pose::Bat => "bat",
            Pose::LookUp => "look_up",
            Pose::Flinch => "flinch",
            Pose::PerkUp => "perk_up",
            Pose::PettedIn => "petted_in",
            Pose::Purr => "purr",
            Pose::Bowl => "bowl",
            Pose::Ball => "ball",
            Pose::Tower => "tower",
        }
    }

    /// Only the cat takes the terminal's colors. The bowl, ball, and cat tree keep their own.
    fn is_cat(self) -> bool {
        !matches!(self, Pose::Bowl | Pose::Ball | Pose::Tower)
    }

    /// The pose's frames as built into the program.
    fn sprites(self) -> &'static [&'static [u8]] {
        SPRITES
            .iter()
            .find(|(folder, _)| *folder == self.folder())
            .map_or(&[], |(_, frames)| frames)
    }

    /// How many frames the pose has.
    pub fn frame_count(self) -> usize {
        self.sprites().len()
    }
}

/// Which way the cat looks. The sprites are drawn facing right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Facing {
    Right,
    Left,
}

/// Frames ready to send, made the first time each one is asked for.
pub struct Frames {
    palette: Option<Palette>,
    /// A frame that needs no change borrows the built in bytes instead of copying them.
    ready: HashMap<(Pose, usize, Facing), Cow<'static, [u8]>>,
}

impl Frames {
    /// Colors the cat with `palette`. Without one, the frames keep their own colors.
    pub fn new(palette: Option<Palette>) -> Self {
        Self {
            palette,
            ready: HashMap::new(),
        }
    }

    /// One frame as a PNG, ready to send. The index wraps around, so a pose can loop.
    pub fn get(&mut self, pose: Pose, index: usize, facing: Facing) -> Result<&[u8]> {
        let sprites = pose.sprites();
        let index = index
            .checked_rem(sprites.len())
            .with_context(|| format!("no frames for {}", pose.folder()))?;
        let key = (pose, index, facing);
        if !self.ready.contains_key(&key) {
            let png = prepare(sprites[index], pose, facing, self.palette)?;
            self.ready.insert(key, png);
        }
        self.ready
            .get(&key)
            .map(|png| png.as_ref())
            .context("a frame was lost right after it was made")
    }
}

/// Colors and flips one built in frame, and writes it back as a PNG.
fn prepare(
    png: &'static [u8],
    pose: Pose,
    facing: Facing,
    palette: Option<Palette>,
) -> Result<Cow<'static, [u8]>> {
    let palette = palette.filter(|_| pose.is_cat());
    if palette.is_none() && facing == Facing::Right {
        // Nothing to change, so send the frame just as it was built in.
        return Ok(Cow::Borrowed(png));
    }
    let mut pixels = pixels::decode(png)?;
    if let Some(palette) = palette {
        palette.apply(&mut pixels.rgba);
    }
    if facing == Facing::Left {
        pixels::flip(&mut pixels);
    }
    Ok(Cow::Owned(pixels::encode(&pixels)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every frame shares one canvas, so a frame can be drawn at a fixed spot.
    const CANVAS: (u32, u32) = (309, CANVAS_HEIGHT);

    const PALETTE: Palette = Palette {
        text: [200, 180, 160],
        background: [10, 20, 30],
    };

    #[test]
    fn every_sprite_folder_is_a_pose_with_frames() {
        for (folder, frames) in SPRITES {
            assert!(
                Pose::ALL.iter().any(|pose| pose.folder() == *folder),
                "{folder} has no pose"
            );
            assert!(!frames.is_empty(), "{folder} has no frames");
        }
        for pose in Pose::ALL {
            assert!(pose.frame_count() > 0, "{pose:?} has no frames");
        }
    }

    #[test]
    fn cat_frames_share_one_canvas() -> Result<()> {
        for pose in Pose::ALL.into_iter().filter(|pose| pose.is_cat()) {
            for png in pose.sprites() {
                let pixels = pixels::decode(png)?;
                assert_eq!((pixels.width, pixels.height), CANVAS, "{pose:?}");
            }
        }
        Ok(())
    }

    #[test]
    fn the_index_wraps_so_poses_can_loop() -> Result<()> {
        let (mut first, mut wrapped) = (Frames::new(None), Frames::new(None));
        assert_eq!(
            first.get(Pose::Sit, 0, Facing::Right)?,
            wrapped.get(Pose::Sit, Pose::Sit.frame_count(), Facing::Right)?
        );
        Ok(())
    }

    #[test]
    fn left_frames_are_mirrored_right_frames() -> Result<()> {
        let mut frames = Frames::new(None);
        let mut right = pixels::decode(frames.get(Pose::Walk, 0, Facing::Right)?)?;
        let left = pixels::decode(frames.get(Pose::Walk, 0, Facing::Left)?)?;
        pixels::flip(&mut right);
        assert_eq!(left.rgba, right.rgba);
        Ok(())
    }

    #[test]
    fn the_cat_takes_the_palette_but_the_bowl_does_not() -> Result<()> {
        let mut plain = Frames::new(None);
        let mut colored = Frames::new(Some(PALETTE));
        let cat = pixels::decode(colored.get(Pose::Sit, 0, Facing::Right)?)?;
        assert!(
            cat.rgba
                .as_chunks::<BYTES_PER_PIXEL>()
                .0
                .iter()
                .filter(|pixel| pixel[3] == u8::MAX)
                .all(|pixel| pixel[..3] != [u8::MAX; 3]),
            "no white should be left on the cat"
        );
        assert_eq!(
            colored.get(Pose::Bowl, 0, Facing::Right)?,
            plain.get(Pose::Bowl, 0, Facing::Right)?
        );
        Ok(())
    }
}
