//! The cat brain: what the cat is doing, where it is, and which frame it shows.
//!
//! The cat follows a list of steps. When it reaches a step that loops, such as sitting or
//! sleeping, it is settled, and it checks the clock to see what it should do next. In free
//! time it rests a while, then picks a resting action at random.

mod plan;
mod step;

use crate::frames::{CANVAS_WIDTH, Facing, Pose, Sprite};
use crate::schedule::{Clock, DayPart};
use plan::{Plan, facing_toward};
use std::collections::VecDeque;
use std::ops::RangeInclusive;
use std::time::{Duration, Instant};
use step::Step;

/// The cat's home spot in the bar, in sprite pixels, just past the bowl. It sleeps here.
pub const HOME_X: u32 = 222;

/// Where the cat sits facing the bowl to eat, in sprite pixels.
const EAT_SPOT: i32 = 263;

/// How long the cat eats at each meal.
const MEAL_LENGTH: Duration = Duration::from_secs(3 * 60);

/// How long the cat keeps looking up after the last key.
const LOOK_HOLD: Duration = Duration::from_millis(900);

/// How long the cat sits still in free time before it picks a resting action, in seconds.
/// Short enough that it does each action within about ten minutes.
const REST_SECONDS: RangeInclusive<u64> = 5..=20;

/// Something the cat does in free time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// Stays sitting, blinking and flicking its tail.
    Sit,
    /// Strolls to a new spot in the bar.
    Walk,
    /// Crouches, wiggles, and pounces.
    Play,
    /// Dashes to the far end of the bar.
    Run,
}

/// Each resting action and how likely it is, as a share of the total.
const ACTIONS: [(Action, u32); 4] = [
    (Action::Sit, 55),
    (Action::Walk, 20),
    (Action::Play, 15),
    (Action::Run, 10),
];

/// What the cat is busy with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Rests, and now and then does a resting action.
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
    /// How wide the bar is, in sprite pixels. The cat stays inside it.
    bar_width: u32,
    /// In free time, when the cat stops resting and picks an action.
    rest_until: Option<Instant>,
    /// When a key was last pressed, so the cat looks up until typing stops.
    last_key: Option<Instant>,
    /// A full screen program such as `vim` is open, so the cat sits still.
    full_screen: bool,
    rng: fastrand::Rng,
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
            bar_width: 0,
            rest_until: None,
            last_key: None,
            full_screen: false,
            rng: fastrand::Rng::new(),
        };
        cat.follow(plan, now);
        cat
    }

    /// What the cat is busy with.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Sets how wide the bar is, in sprite pixels, when the window opens or changes size.
    pub fn set_bar_width(&mut self, width: u32) {
        self.bar_width = width;
    }

    /// Notes that a full screen program such as `vim` opened or closed.
    pub fn set_full_screen(&mut self, full_screen: bool) {
        self.full_screen = full_screen;
    }

    /// A key was pressed. A cat sitting in free time looks up. A sleeping or eating cat,
    /// or one busy with an action, does not notice.
    pub fn key_pressed(&mut self, now: Instant) {
        self.last_key = Some(now);
        if self.mode != Mode::Free || self.full_screen || !self.settled() {
            return;
        }
        let step = *self.step();
        if step.pose == Pose::Sit {
            self.follow(Plan::from(step.x, step.facing).look_up(), now);
        }
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
        if self.looking_up() {
            self.look_back_down(now);
        }
        if self.settled() {
            self.check_the_clock(now);
        }
        // The clock may have given the cat something to do, so check it is still settled.
        if self.mode == Mode::Free && self.settled() {
            self.rest(now);
        }
    }

    /// The cat is sitting or sleeping with nothing else planned, so it can start something.
    fn settled(&self) -> bool {
        let step = self.step();
        step.frames.is_none() && matches!(step.pose, Pose::Sit | Pose::Sleep)
    }

    /// The cat is holding its look up at the keys.
    fn looking_up(&self) -> bool {
        let step = self.step();
        step.frames.is_none() && step.pose == Pose::LookUp
    }

    /// Once typing has stopped for a moment, looks back down and rests again.
    fn look_back_down(&mut self, now: Instant) {
        let typing = self
            .last_key
            .is_some_and(|key| now.saturating_duration_since(key) < LOOK_HOLD);
        if typing {
            return;
        }
        let step = *self.step();
        self.rest_until = None;
        self.follow(Plan::from(step.x, step.facing).look_down(), now);
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
        self.rest_until = None;
        self.follow(plan, now);
    }

    /// In free time, rests a while, then picks a resting action.
    fn rest(&mut self, now: Instant) {
        if self.full_screen {
            // The cat sits still, and rests from scratch once the program closes.
            self.rest_until = None;
            return;
        }
        let Some(until) = self.rest_until else {
            let seconds = self.rng.u64(REST_SECONDS);
            self.rest_until = Some(now + Duration::from_secs(seconds));
            return;
        };
        if now < until {
            return;
        }
        // The next rest starts once the action is done and the cat is settled again.
        self.rest_until = None;
        let action = self.pick_action();
        self.act(action, now);
    }

    /// Picks a resting action at random, by weight.
    fn pick_action(&mut self) -> Action {
        let total: u32 = ACTIONS.iter().map(|(_, weight)| weight).sum();
        let mut roll = self.rng.u32(0..total);
        for (action, weight) in ACTIONS {
            if roll < weight {
                return action;
            }
            roll -= weight;
        }
        Action::Sit
    }

    /// Plans a resting action from where the cat sits. Without room to move, it stays put.
    fn act(&mut self, action: Action, now: Instant) {
        let step = *self.step();
        let (x, facing) = (step.x, step.facing);
        let (left, right) = (home_x(), self.right_edge());
        if right <= left {
            return;
        }
        let plan = Plan::from(x, facing);
        let plan = match action {
            Action::Sit => return,
            Action::Walk => {
                let to = self.rng.i32(left..=right);
                plan.go_to(to, facing_toward(to - x)).sit()
            }
            Action::Run => {
                // Dash to whichever end is farther away.
                let to = if x - left > right - x { left } else { right };
                plan.get_up().run_to(to).sit_down().sit()
            }
            Action::Play => {
                let ahead = match facing {
                    Facing::Right => x + Plan::pounce_length(),
                    Facing::Left => x - Plan::pounce_length(),
                };
                // Without room ahead, it turns and pounces the other way.
                let facing = if (left..=right).contains(&ahead) {
                    facing
                } else {
                    facing.opposite()
                };
                plan.pounce(facing)
            }
        };
        self.follow(plan, now);
    }

    /// The farthest spot right the cat can be drawn at with all of it in the bar.
    fn right_edge(&self) -> i32 {
        let edge = self.bar_width.saturating_sub(CANVAS_WIDTH);
        i32::try_from(edge).unwrap_or(i32::MAX)
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
    still: false,
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

    /// The bar in an 80 column window with cells twice as tall as wide.
    const BAR_WIDTH: u32 = 4300;

    /// A cat in free time, on a real speed clock so no meal or nap comes along.
    fn free_cat(now: Instant, seed: u64) -> Cat {
        let mut cat = Cat::new(now, Clock::starting_at(time(10, 0, 0, 0), 1, now));
        cat.rng = fastrand::Rng::with_seed(seed);
        cat.set_bar_width(BAR_WIDTH);
        cat
    }

    /// Ticks the cat frame by frame for `length`, and returns every pose it showed and the
    /// spots it was drawn at.
    fn watch(cat: &mut Cat, start: Instant, length: Duration) -> (Vec<Pose>, Vec<u32>) {
        let (mut poses, mut spots) = (Vec::new(), Vec::new());
        let mut now = start;
        while now < start + length {
            now = cat.next_change(now);
            cat.tick(now);
            let sprite = cat.sprite(now);
            poses.push(sprite.pose);
            spots.push(sprite.x);
        }
        (poses, spots)
    }

    #[test]
    fn left_alone_in_free_time_the_cat_does_each_action() {
        let start = Instant::now();
        let mut cat = free_cat(start, 1);
        let (poses, _) = watch(&mut cat, start, Duration::from_secs(10 * 60));
        for pose in [Pose::Sit, Pose::Walk, Pose::Pounce, Pose::Run] {
            assert!(poses.contains(&pose), "{pose:?} never happened");
        }
        assert_eq!(cat.mode(), Mode::Free);
    }

    #[test]
    fn the_cat_stays_inside_the_bar() {
        let start = Instant::now();
        for seed in 0..5 {
            let mut cat = free_cat(start, seed);
            let (_, spots) = watch(&mut cat, start, Duration::from_secs(10 * 60));
            // A turn moves the drawn spot by a few pixels either side.
            let right_edge = BAR_WIDTH - CANVAS_WIDTH + 6;
            assert!(
                spots.iter().all(|x| *x + 6 >= HOME_X && *x <= right_edge),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn the_cat_rests_before_it_acts() {
        let start = Instant::now();
        let mut cat = free_cat(start, 1);
        let (poses, _) = watch(&mut cat, start, Duration::from_secs(*REST_SECONDS.start()));
        assert!(poses.iter().all(|pose| *pose == Pose::Sit));
    }

    #[test]
    fn without_a_bar_width_the_cat_stays_put() {
        let start = Instant::now();
        let mut cat = Cat::new(start, Clock::starting_at(time(10, 0, 0, 0), 1, start));
        let (_, spots) = watch(&mut cat, start, Duration::from_secs(5 * 60));
        assert!(spots.iter().all(|x| *x == HOME_X));
    }

    #[test]
    fn sitting_is_picked_most_often() {
        let mut cat = free_cat(Instant::now(), 7);
        let picks: Vec<Action> = (0..1000).map(|_| cat.pick_action()).collect();
        let count = |action| picks.iter().filter(|pick| **pick == action).count();
        assert!(count(Action::Sit) > count(Action::Walk));
        assert!(count(Action::Walk) > count(Action::Run));
        assert!(count(Action::Run) > 0);
    }

    #[test]
    fn typing_makes_a_sitting_cat_look_up_until_it_stops() {
        let start = Instant::now();
        let mut cat = free_cat(start, 1);
        cat.key_pressed(start);
        let (poses, _) = watch(&mut cat, start, Duration::from_millis(500));
        assert_eq!(poses.last(), Some(&Pose::LookUp));

        // Keys keep coming, so the cat keeps looking.
        let mut now = start;
        for _ in 0..5 {
            now += Duration::from_millis(500);
            cat.key_pressed(now);
            cat.tick(now);
            assert_eq!(cat.sprite(now).pose, Pose::LookUp);
        }

        // Typing stops, so it looks back down and sits.
        let (poses, _) = watch(&mut cat, now, LOOK_HOLD + Duration::from_secs(1));
        assert_eq!(poses.last(), Some(&Pose::Sit));
    }

    #[test]
    fn typing_does_not_wake_a_sleeping_cat() {
        let start = Instant::now();
        let mut cat = cat_at(2, 0, start);
        cat.key_pressed(start);
        let (poses, _) = watch(&mut cat, start, Duration::from_secs(3));
        assert!(poses.iter().all(|pose| *pose == Pose::Sleep));
    }

    #[test]
    fn in_a_full_screen_program_the_cat_sits_still() {
        let start = Instant::now();
        let mut cat = free_cat(start, 1);
        cat.set_full_screen(true);
        cat.key_pressed(start);
        let (poses, spots) = watch(&mut cat, start, Duration::from_secs(10 * 60));
        assert!(poses.iter().all(|pose| *pose == Pose::Sit));
        assert!(spots.iter().all(|x| *x == HOME_X));

        // Once the program closes, the cat goes back to its resting actions.
        cat.set_full_screen(false);
        let later = start + Duration::from_secs(10 * 60);
        let (poses, _) = watch(&mut cat, later, Duration::from_secs(10 * 60));
        assert!(poses.iter().any(|pose| *pose != Pose::Sit));
    }
}
