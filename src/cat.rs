//! The cat brain: what the cat is doing, where it is, and which frame it shows.
//!
//! The cat follows a list of steps. When it reaches a step that loops, such as sitting or
//! sleeping, it is settled, and it checks the clock to see what it should do next.

mod plan;
mod step;

use crate::frames::{Facing, Pose, Sprite};
use crate::schedule::{Clock, DayPart};
use plan::Plan;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use step::Step;

/// The cat's home spot in the bar, in sprite pixels, just past the bowl. It sleeps here.
pub const HOME_X: u32 = 222;

/// Where the cat sits facing the bowl to eat, in sprite pixels.
const EAT_SPOT: i32 = 263;

/// How long the cat eats at each meal.
const MEAL_LENGTH: Duration = Duration::from_secs(3 * 60);

/// What the cat is busy with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Sits about. Resting actions come later.
    Free,
    Eating,
    Sleeping,
}

/// The cat, what it is doing, and when it started its current step.
pub struct Cat {
    clock: Clock,
    mode: Mode,
    /// What is left to do. The first step is the one on screen. Never empty.
    steps: VecDeque<Step>,
    /// When the first step started.
    started: Instant,
    /// The meal the cat already ate, so it eats once per meal time.
    fed: Option<DayPart>,
}

impl Cat {
    /// A cat in a window that opens at `now`. At nap or night time it is already asleep.
    pub fn new(now: Instant, clock: Clock) -> Self {
        let home = home_x();
        let (mode, plan) = if DayPart::at(clock.time(now)).is_sleep() {
            (Mode::Sleeping, Plan::from(home, Facing::Right).asleep())
        } else {
            (Mode::Free, Plan::from(home, Facing::Right).sit())
        };
        let mut cat = Self {
            clock,
            mode,
            steps: VecDeque::new(),
            started: now,
            fed: None,
        };
        cat.follow(plan, now);
        cat
    }

    /// What the cat is busy with.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Moves the cat on to `now`: finishes steps that are done, and when it is settled,
    /// checks the clock for something new to do.
    pub fn tick(&mut self, now: Instant) {
        while self.steps.len() > 1 {
            let Some(duration) = self.step().duration() else {
                break;
            };
            let Some(ends) = self
                .started
                .checked_add(duration)
                .filter(|ends| *ends <= now)
            else {
                break;
            };
            self.steps.pop_front();
            // The next step starts when the last one ended, so slow ticks do not slow the cat.
            self.started = ends;
        }
        if self.step().frames.is_none() {
            self.check_the_clock(now);
        }
    }

    /// The frame to show at `now`.
    pub fn sprite(&self, now: Instant) -> Sprite {
        self.step().sprite(self.frames_shown(now))
    }

    /// When the frame shown at `now` changes to the next one.
    pub fn next_change(&self, now: Instant) -> Instant {
        let frame_time = self.step().pose.frame_time();
        let next = self.frames_shown(now).saturating_add(1);
        let since_start = u32::try_from(next).map_or(Duration::MAX, |next| frame_time * next);
        self.started.checked_add(since_start).unwrap_or(now)
    }

    /// Picks the mode for this time of day, and plans the way there.
    fn check_the_clock(&mut self, now: Instant) {
        let part = DayPart::at(self.clock.time(now));
        if !part.is_meal() {
            self.fed = None;
        }
        let wanted = if part.is_sleep() {
            Mode::Sleeping
        } else if part.is_meal() && self.fed != Some(part) {
            Mode::Eating
        } else {
            Mode::Free
        };
        if wanted == self.mode {
            return;
        }

        let step = *self.step();
        let mut plan = Plan::from(step.x, step.facing);
        if step.pose == Pose::Sleep {
            plan = plan.wake();
        }
        plan = match wanted {
            Mode::Free => plan.sit(),
            Mode::Eating => {
                self.fed = Some(part);
                plan.go_to(EAT_SPOT, Facing::Left).eat(MEAL_LENGTH)
            }
            Mode::Sleeping => plan.go_to(home_x(), Facing::Right).sleep(),
        };
        self.mode = wanted;
        self.follow(plan, now);
    }

    /// Starts on a new list of steps at `now`.
    fn follow(&mut self, plan: Plan, now: Instant) {
        let steps = plan.steps();
        if steps.is_empty() {
            return;
        }
        self.steps = steps.into();
        self.started = now;
    }

    /// The step on screen now.
    fn step(&self) -> &Step {
        // `steps` is never empty: `follow` only takes a plan with steps, and `tick` keeps
        // the last one.
        self.steps.front().unwrap_or(&IDLE)
    }

    /// How many frames of the current step have fully played by `now`.
    fn frames_shown(&self, now: Instant) -> usize {
        let elapsed = now.saturating_duration_since(self.started).as_millis();
        let frames = elapsed / self.step().pose.frame_time().as_millis().max(1);
        usize::try_from(frames).unwrap_or(usize::MAX)
    }
}

/// Shown only if the steps ever ran out, which `follow` and `tick` do not allow.
const IDLE: Step = Step {
    pose: Pose::Sit,
    facing: Facing::Right,
    backward: false,
    frames: None,
    x: 0,
    move_per_frame: 0,
};

/// The home spot as a signed spot, for planning moves.
fn home_x() -> i32 {
    i32::try_from(HOME_X).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::time;

    /// The clock runs this many times faster in tests, so a minute passes in a second.
    const SPEED: u32 = 60;

    fn cat_at(hour: i8, minute: i8, now: Instant) -> Cat {
        Cat::new(
            now,
            Clock::starting_at(time(hour, minute, 0, 0), SPEED, now),
        )
    }

    /// Ticks the cat frame by frame until `until`, and returns the time it got to.
    fn run(cat: &mut Cat, mut now: Instant, until: Instant) -> Instant {
        while now < until {
            now = cat.next_change(now).min(until);
            cat.tick(now);
        }
        now
    }

    #[test]
    fn in_free_time_the_cat_sits_at_home() {
        let now = Instant::now();
        let cat = cat_at(10, 0, now);
        let sprite = cat.sprite(now);
        assert_eq!(cat.mode(), Mode::Free);
        assert_eq!(
            (sprite.pose, sprite.x, sprite.facing),
            (Pose::Sit, HOME_X, Facing::Right)
        );
    }

    #[test]
    fn a_window_opened_at_night_finds_the_cat_asleep() {
        let now = Instant::now();
        let cat = cat_at(2, 0, now);
        assert_eq!(cat.mode(), Mode::Sleeping);
        assert_eq!(cat.sprite(now).pose, Pose::Sleep);
    }

    #[test]
    fn at_breakfast_the_cat_eats_then_sits_by_the_bowl() {
        let start = Instant::now();
        let mut cat = cat_at(7, 30, start);
        cat.tick(start);
        assert_eq!(cat.mode(), Mode::Eating);

        let now = run(&mut cat, start, start + Duration::from_secs(10));
        let sprite = cat.sprite(now);
        assert_eq!((sprite.pose, sprite.facing), (Pose::Eat, Facing::Left));

        let now = run(&mut cat, now, now + MEAL_LENGTH);
        let sprite = cat.sprite(now);
        assert_eq!(cat.mode(), Mode::Free);
        assert_eq!(sprite.pose, Pose::Sit);
        assert_eq!(sprite.x, u32::try_from(EAT_SPOT).unwrap_or(0));
        assert_eq!(sprite.facing, Facing::Left);
    }

    #[test]
    fn a_fed_cat_does_not_eat_again_in_the_same_meal() {
        let start = Instant::now();
        let mut cat = cat_at(7, 0, start);
        let now = run(
            &mut cat,
            start,
            start + MEAL_LENGTH + Duration::from_secs(10),
        );
        assert_eq!(cat.mode(), Mode::Free);
        // Still breakfast on the clock: about 3 hours at 60 times speed is not reached.
        run(&mut cat, now, now + Duration::from_secs(30));
        assert_eq!(cat.mode(), Mode::Free);
    }

    #[test]
    fn at_nap_time_the_cat_goes_home_and_sleeps_then_wakes() {
        let start = Instant::now();
        // Breakfast ends at 9:00, a minute in. Then it is free, and nap time is far off.
        let mut cat = Cat::new(start, Clock::starting_at(time(12, 59, 0, 0), SPEED, start));
        let now = run(&mut cat, start, start + Duration::from_secs(10));
        let sprite = cat.sprite(now);
        assert_eq!(cat.mode(), Mode::Sleeping);
        assert_eq!((sprite.pose, sprite.x), (Pose::Sleep, HOME_X));

        // The nap ends at 15:00, two clock hours later.
        let now = run(&mut cat, now, now + Duration::from_secs(2 * 60 + 10));
        assert_eq!(cat.mode(), Mode::Free);
        assert_eq!(cat.sprite(now).pose, Pose::Sit);
    }

    #[test]
    fn a_cat_away_from_home_walks_back_to_sleep() {
        let start = Instant::now();
        // Dinner at 18:00, then night at 23:00, five clock hours later.
        let mut cat = cat_at(18, 0, start);
        let now = run(&mut cat, start, start + Duration::from_secs(5 * 60 + 30));
        let sprite = cat.sprite(now);
        assert_eq!(cat.mode(), Mode::Sleeping);
        assert_eq!(
            (sprite.pose, sprite.x, sprite.facing),
            (Pose::Sleep, HOME_X, Facing::Right)
        );
    }

    #[test]
    fn the_next_change_is_the_start_of_the_next_frame() {
        let start = Instant::now();
        let cat = cat_at(10, 0, start);
        let frame_time = Pose::Sit.frame_time();

        assert_eq!(cat.next_change(start), start + frame_time);
        assert_eq!(cat.next_change(start + frame_time / 2), start + frame_time);
        assert_eq!(cat.next_change(start + frame_time), start + frame_time * 2);
    }
}
