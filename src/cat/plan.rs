//! Builds the steps for each thing the cat does, such as walking somewhere or lying down.
//!
//! The spots and distances match the kitty test player, `assets/tools/play_in_kitty.py`.

use super::step::Step;
use crate::frames::{Facing, Pose};
use std::time::Duration;

/// How far the cat moves with each walk frame, in sprite pixels.
const WALK_STRIDE: i32 = 13;

/// How far the cat moves with each run frame, in sprite pixels.
const RUN_STRIDE: i32 = 39;

/// How far the cat moves with each pounce frame, in sprite pixels.
const POUNCE_STEP: i32 = 14;

/// Times the cat wiggles its rear before it pounces.
const WIGGLES: usize = 2;

/// Turning around flips the frames, which moves the body this far on its canvas when it
/// ends up facing left. The draw spot moves by this much so the cat turns in place.
const FLIP_SHIFT: i32 = -6;

/// How far the cat scoots toward the bowl with each lean frame, in sprite pixels.
const LEAN_STEP: i32 = -16;

/// How far from its seat the cat eats, so its nose reaches into the bowl.
const EAT_OFFSET: i32 = -64;

/// Steps for the cat, built from where it is and which way it faces.
pub struct Plan {
    steps: Vec<Step>,
    x: i32,
    facing: Facing,
}

impl Plan {
    /// Starts a plan for a cat at `x` facing `facing`.
    pub fn from(x: i32, facing: Facing) -> Self {
        Self {
            steps: Vec::new(),
            x,
            facing,
        }
    }

    /// The steps, in the order the cat does them.
    pub fn steps(self) -> Vec<Step> {
        self.steps
    }

    /// Plays every frame of `pose` once, in place.
    fn play(&mut self, pose: Pose, backward: bool) {
        self.steps.push(Step {
            pose,
            facing: self.facing,
            backward,
            frames: Some(pose.frame_count()),
            x: self.x,
            move_per_frame: 0,
        });
    }

    /// Loops `pose` in place until the cat does something else.
    fn hold(&mut self, pose: Pose) {
        self.steps.push(Step {
            pose,
            facing: self.facing,
            backward: false,
            frames: None,
            x: self.x,
            move_per_frame: 0,
        });
    }

    /// From sitting, stands up: the sit down frames played backward.
    pub fn get_up(mut self) -> Self {
        self.play(Pose::SitDown, true);
        self
    }

    /// From standing, sits down.
    pub fn sit_down(mut self) -> Self {
        self.play(Pose::SitDown, false);
        self
    }

    /// Sits, blinking and flicking its tail, until it does something else.
    pub fn sit(mut self) -> Self {
        self.hold(Pose::Sit);
        self
    }

    /// From standing, turns to face `facing`: halfway round to face you, then on round.
    pub fn turn_to(mut self, facing: Facing) -> Self {
        if facing == self.facing {
            return self;
        }
        self.play(Pose::Turn, false);
        self.x += turn_shift(facing);
        self.facing = facing;
        self.play(Pose::Turn, true);
        self
    }

    /// From standing, turns if needed and walks to `x`.
    pub fn walk_to(self, x: i32) -> Self {
        self.travel(Pose::Walk, WALK_STRIDE, x)
    }

    /// From standing, turns if needed and runs to `x`.
    pub fn run_to(self, x: i32) -> Self {
        self.travel(Pose::Run, RUN_STRIDE, x)
    }

    /// From standing, turns to face `x` and moves there with `pose`, `stride` pixels a frame.
    fn travel(mut self, pose: Pose, stride: i32, x: i32) -> Self {
        let distance = x - self.x;
        if distance == 0 {
            return self;
        }
        self = self.turn_to(facing_toward(distance));
        // Turning moved the spot a little, so measure again from there.
        let distance = x - self.x;
        let frames = distance
            .unsigned_abs()
            .div_ceil(stride.unsigned_abs())
            .max(1);
        let move_per_frame = distance / i32::try_from(frames).unwrap_or(i32::MAX);
        self.steps.push(Step {
            pose,
            facing: self.facing,
            backward: false,
            frames: Some(usize::try_from(frames).unwrap_or(usize::MAX)),
            // The first frame is already a stride out, so the last one lands on the spot.
            x: self.x + move_per_frame,
            move_per_frame,
        });
        self.x = x;
        self
    }

    /// From sitting, gets up, walks to `x`, turns to `facing`, and sits down. Does nothing
    /// when it is already there.
    pub fn go_to(self, x: i32, facing: Facing) -> Self {
        if x == self.x && facing == self.facing {
            return self;
        }
        let walking = facing_toward(x - self.x);
        // A turn at the end moves the spot, so stop short by that much.
        let last_turn = if walking == facing {
            0
        } else {
            turn_shift(facing)
        };
        self.get_up()
            .walk_to(x - last_turn)
            .turn_to(facing)
            .sit_down()
    }

    /// From sitting at the bowl, scoots in, eats for about `length`, scoots back, and sits.
    pub fn eat(mut self, length: Duration) -> Self {
        let seat = self.x;
        let lean_frames = Pose::Lean.frame_count();
        let lean_frames_signed = i32::try_from(lean_frames).unwrap_or(i32::MAX);
        let mut lean = Step {
            pose: Pose::Lean,
            facing: self.facing,
            backward: false,
            frames: Some(lean_frames),
            x: seat + LEAN_STEP,
            move_per_frame: LEAN_STEP,
        };
        self.steps.push(lean);
        let eat_frames = length.as_millis() / Pose::Eat.frame_time().as_millis().max(1);
        self.steps.push(Step {
            pose: Pose::Eat,
            facing: self.facing,
            backward: false,
            frames: Some(usize::try_from(eat_frames).unwrap_or(usize::MAX).max(1)),
            x: seat + EAT_OFFSET,
            move_per_frame: 0,
        });
        lean.backward = true;
        lean.x = seat + LEAN_STEP * lean_frames_signed;
        lean.move_per_frame = -LEAN_STEP;
        self.steps.push(lean);
        self.sit()
    }

    /// From sitting, gets up, turns to `facing`, crouches, wiggles, pounces, bats, and sits.
    pub fn pounce(mut self, facing: Facing) -> Self {
        self = self.get_up().turn_to(facing);
        self.play(Pose::Crouch, false);
        for _ in 0..WIGGLES {
            self.play(Pose::Wiggle, false);
        }
        let frames = Pose::Pounce.frame_count();
        let move_per_frame = match facing {
            Facing::Right => POUNCE_STEP,
            Facing::Left => -POUNCE_STEP,
        };
        self.steps.push(Step {
            pose: Pose::Pounce,
            facing,
            backward: false,
            frames: Some(frames),
            x: self.x + move_per_frame,
            move_per_frame,
        });
        self.x += move_per_frame * i32::try_from(frames).unwrap_or(0);
        self.play(Pose::Bat, false);
        self.play(Pose::Crouch, true);
        self.sit_down().sit()
    }

    /// How far a pounce moves the cat, so the caller can check there is room.
    pub fn pounce_length() -> i32 {
        POUNCE_STEP * i32::try_from(Pose::Pounce.frame_count()).unwrap_or(0)
    }

    /// From sitting, lies down and falls asleep: getting up from a nap, played backward.
    pub fn sleep(mut self) -> Self {
        self.play(Pose::Stand, true);
        self.play(Pose::Wake, true);
        self.hold(Pose::Sleep);
        self
    }

    /// Wakes from sleep and gets up to sitting.
    pub fn wake(mut self) -> Self {
        self.play(Pose::Wake, false);
        self.play(Pose::Stand, false);
        self
    }

    /// Sleeps, for a cat that is asleep already.
    pub fn asleep(mut self) -> Self {
        self.hold(Pose::Sleep);
        self
    }
}

/// The way to face to move `distance`, which is negative to the left.
pub fn facing_toward(distance: i32) -> Facing {
    if distance >= 0 {
        Facing::Right
    } else {
        Facing::Left
    }
}

/// How far the draw spot moves when the cat turns to face `facing`, so it turns in place.
fn turn_shift(facing: Facing) -> i32 {
    match facing {
        Facing::Left => FLIP_SHIFT,
        Facing::Right => -FLIP_SHIFT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poses(plan: Plan) -> Vec<(Pose, bool)> {
        plan.steps()
            .iter()
            .map(|step| (step.pose, step.backward))
            .collect()
    }

    #[test]
    fn going_somewhere_gets_up_walks_and_sits() {
        let plan = Plan::from(100, Facing::Right)
            .go_to(200, Facing::Right)
            .sit();
        assert_eq!(
            poses(plan),
            [
                (Pose::SitDown, true),
                (Pose::Walk, false),
                (Pose::SitDown, false),
                (Pose::Sit, false)
            ]
        );
    }

    #[test]
    fn going_back_turns_around_first() -> anyhow::Result<()> {
        let steps = Plan::from(200, Facing::Right)
            .go_to(100, Facing::Left)
            .steps();
        let walk = steps
            .iter()
            .find(|step| step.pose == Pose::Walk)
            .ok_or_else(|| anyhow::anyhow!("the cat should walk"))?;
        assert_eq!(walk.facing, Facing::Left);
        assert!(walk.move_per_frame < 0);
        assert_eq!(
            steps.iter().filter(|step| step.pose == Pose::Turn).count(),
            2
        );
        Ok(())
    }

    #[test]
    fn a_walk_ends_near_its_spot() -> anyhow::Result<()> {
        let steps = Plan::from(100, Facing::Right).walk_to(150).steps();
        let walk = steps.first().ok_or_else(|| anyhow::anyhow!("a walk"))?;
        let frames = walk.frames.ok_or_else(|| anyhow::anyhow!("walks end"))?;
        let first = i32::try_from(walk.sprite(0).x)?;
        let last = i32::try_from(walk.sprite(frames - 1).x)?;
        assert!(first > 100, "the first frame steps out");
        // Only the rounding of the stride is left over, less than one pixel per frame.
        assert!((150 - i32::try_from(frames)?..=150).contains(&last));
        Ok(())
    }

    #[test]
    fn a_turn_at_the_end_still_lands_on_the_spot() {
        for (from, facing_from, to, facing_to) in [
            (222, Facing::Right, 263, Facing::Left),
            (263, Facing::Left, 222, Facing::Right),
        ] {
            let plan = Plan::from(from, facing_from).go_to(to, facing_to);
            assert_eq!((plan.x, plan.facing), (to, facing_to));
        }
    }

    #[test]
    fn a_long_walk_ends_on_its_spot() -> anyhow::Result<()> {
        let plan = Plan::from(100, Facing::Right).walk_to(1000);
        assert_eq!(plan.x, 1000);
        Ok(())
    }

    #[test]
    fn running_takes_fewer_frames_than_walking() {
        let frames = |plan: Plan| {
            plan.steps()
                .iter()
                .filter_map(|step| step.frames)
                .sum::<usize>()
        };
        let walk = frames(Plan::from(0, Facing::Right).walk_to(1000));
        let run = frames(Plan::from(0, Facing::Right).run_to(1000));
        assert!(run * 2 < walk);
    }

    #[test]
    fn a_pounce_lands_ahead_and_ends_sitting() {
        let plan = Plan::from(500, Facing::Left).pounce(Facing::Left);
        assert_eq!(plan.x, 500 - Plan::pounce_length());
        let steps = plan.steps();
        let poses: Vec<Pose> = steps.iter().map(|step| step.pose).collect();
        assert_eq!(poses.first(), Some(&Pose::SitDown));
        assert!(poses.contains(&Pose::Pounce) && poses.contains(&Pose::Bat));
        assert_eq!(poses.last(), Some(&Pose::Sit));
    }

    #[test]
    fn a_cat_already_there_stays_put() {
        assert!(
            Plan::from(100, Facing::Left)
                .go_to(100, Facing::Left)
                .steps()
                .is_empty()
        );
    }

    #[test]
    fn eating_scoots_in_and_back_to_its_seat() -> anyhow::Result<()> {
        let steps = Plan::from(263, Facing::Left)
            .eat(Duration::from_secs(1))
            .steps();
        let [lean_in, eat, lean_out, sit] = steps.as_slice() else {
            anyhow::bail!("eating has four steps");
        };
        assert_eq!(lean_in.sprite(0).x, 247);
        assert_eq!(lean_in.sprite(2).x, 215);
        assert_eq!(eat.sprite(0).x, 199);
        assert_eq!(lean_out.sprite(0).x, 215);
        assert_eq!(lean_out.sprite(2).x, 247);
        assert_eq!(sit.sprite(0).x, 263);
        Ok(())
    }
}
