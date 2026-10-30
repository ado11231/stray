//! One step of what the cat does: a pose played for some frames, maybe while it moves.

use crate::frames::{Facing, Pose, Sprite};
use std::time::Duration;

/// A pose played from a spot, moving the same distance with each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub pose: Pose,
    pub facing: Facing,
    /// Plays the frames from last to first, such as getting up with the sit down frames.
    pub backward: bool,
    /// How many frames to show, or `None` to loop until the cat does something else.
    pub frames: Option<usize>,
    /// Where the first frame is drawn, in sprite pixels from the left of the bar.
    pub x: i32,
    /// How far each frame is drawn from the one before. Negative moves left.
    pub move_per_frame: i32,
    /// Keeps showing the first frame instead of playing on, such as a held look up.
    pub still: bool,
}

impl Step {
    /// How long the step takes, or `None` when it loops.
    pub fn duration(&self) -> Option<Duration> {
        let frames = u32::try_from(self.frames?).unwrap_or(u32::MAX);
        Some(self.pose.frame_time().saturating_mul(frames))
    }

    /// The sprite to show after `shown` frames of this step have played.
    pub fn sprite(&self, shown: usize) -> Sprite {
        // A step that has run out holds its last frame.
        let shown = match self.frames {
            _ if self.still => 0,
            Some(frames) => shown.min(frames.saturating_sub(1)),
            None => shown,
        };
        let count = self.pose.frame_count().max(1);
        let index = if self.backward {
            count - 1 - shown % count
        } else {
            shown % count
        };
        let moved = i32::try_from(shown)
            .unwrap_or(i32::MAX)
            .saturating_mul(self.move_per_frame);
        Sprite {
            pose: self.pose,
            index,
            facing: self.facing,
            x: u32::try_from(self.x.saturating_add(moved)).unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GET_UP: Step = Step {
        pose: Pose::SitDown,
        facing: Facing::Right,
        backward: true,
        frames: Some(4),
        x: 100,
        move_per_frame: 0,
        still: false,
    };

    #[test]
    fn backward_steps_play_from_the_last_frame() {
        let last = Pose::SitDown.frame_count() - 1;
        assert_eq!(GET_UP.sprite(0).index, last);
        assert_eq!(GET_UP.sprite(1).index, last - 1);
    }

    #[test]
    fn a_finished_step_holds_its_last_frame() {
        assert_eq!(GET_UP.sprite(10), GET_UP.sprite(3));
        assert_eq!(GET_UP.duration(), Some(Pose::SitDown.frame_time() * 4));
    }

    #[test]
    fn a_moving_step_moves_with_each_frame() {
        let walk = Step {
            pose: Pose::Walk,
            frames: Some(5),
            move_per_frame: -13,
            backward: false,
            ..GET_UP
        };
        assert_eq!(walk.sprite(0).x, 100);
        assert_eq!(walk.sprite(2).x, 74);
    }

    #[test]
    fn a_spot_left_of_the_bar_is_drawn_at_its_edge() {
        let step = Step { x: -20, ..GET_UP };
        assert_eq!(step.sprite(0).x, 0);
    }

    #[test]
    fn a_still_step_keeps_its_first_frame() {
        let held = Step {
            still: true,
            frames: None,
            ..GET_UP
        };
        assert_eq!(held.sprite(0), held.sprite(50));
        assert_eq!(held.sprite(0).index, Pose::SitDown.frame_count() - 1);
    }

    #[test]
    fn a_looping_step_has_no_end() {
        let sit = Step {
            pose: Pose::Sit,
            frames: None,
            backward: false,
            ..GET_UP
        };
        assert_eq!(sit.duration(), None);
        assert_eq!(sit.sprite(Pose::Sit.frame_count()).index, 0);
    }
}
