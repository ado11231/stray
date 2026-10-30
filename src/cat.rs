//! The cat brain: what the cat is doing, where it is, and which frame it shows.

use crate::frames::{Facing, Pose, Sprite};
use std::time::{Duration, Instant};

/// The cat's home spot in the bar, in sprite pixels, just past the bowl.
pub const HOME_X: u32 = 222;

/// The cat, and when it started what it is doing now.
pub struct Cat {
    started: Instant,
}

impl Cat {
    pub fn new(now: Instant) -> Self {
        Self { started: now }
    }

    /// The frame to show at `now`. For now the cat sits at home, blinking and flicking its tail.
    pub fn sprite(&self, now: Instant) -> Sprite {
        Sprite {
            pose: Pose::Sit,
            index: self.frames_shown(now, Pose::Sit),
            facing: Facing::Right,
            x: HOME_X,
        }
    }

    /// When the frame shown at `now` changes to the next one.
    pub fn next_change(&self, now: Instant) -> Instant {
        let frame_time = Pose::Sit.frame_time();
        let next = self.frames_shown(now, Pose::Sit) + 1;
        let since_start = u32::try_from(next).map_or(Duration::MAX, |next| frame_time * next);
        self.started.checked_add(since_start).unwrap_or(now)
    }

    /// How many frames of `pose` have fully played since the cat started.
    fn frames_shown(&self, now: Instant, pose: Pose) -> usize {
        let elapsed = now.saturating_duration_since(self.started).as_millis();
        let frames = elapsed / pose.frame_time().as_millis().max(1);
        usize::try_from(frames).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sit_frames_play_in_order_at_home() {
        let start = Instant::now();
        let cat = Cat::new(start);
        let frame_time = Pose::Sit.frame_time();

        assert_eq!(cat.sprite(start).index, 0);
        assert_eq!(cat.sprite(start + frame_time / 2).index, 0);
        assert_eq!(cat.sprite(start + frame_time).index, 1);
        assert_eq!(cat.sprite(start + frame_time * 3).index, 3);
        assert_eq!(cat.sprite(start).x, HOME_X);
    }

    #[test]
    fn the_next_change_is_the_start_of_the_next_frame() {
        let start = Instant::now();
        let cat = Cat::new(start);
        let frame_time = Pose::Sit.frame_time();

        assert_eq!(cat.next_change(start), start + frame_time);
        assert_eq!(cat.next_change(start + frame_time / 2), start + frame_time);
        assert_eq!(cat.next_change(start + frame_time), start + frame_time * 2);
    }
}
