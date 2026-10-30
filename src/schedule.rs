//! The cat's day: turns the time on the clock into meal, nap, night, or free time.
//!
//! Every window reads the same clock, so every cat keeps the same day without saving
//! anything. For testing, the clock can start at another time and run faster.

use crate::env;
use anyhow::{Context, Result};
use jiff::SignedDuration;
use jiff::civil::{Time, time};
use std::time::Instant;

/// A part of the cat's day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayPart {
    Breakfast,
    Nap,
    Dinner,
    Night,
    /// Any time not in the schedule: the cat wanders and picks resting actions.
    Free,
}

/// One part of the day, from its start up to (not including) its end.
struct Period {
    start: Time,
    end: Time,
    part: DayPart,
}

/// The cat's day. A period whose end comes before its start runs past midnight.
const SCHEDULE: [Period; 4] = [
    Period {
        start: time(7, 0, 0, 0),
        end: time(9, 0, 0, 0),
        part: DayPart::Breakfast,
    },
    Period {
        start: time(13, 0, 0, 0),
        end: time(15, 0, 0, 0),
        part: DayPart::Nap,
    },
    Period {
        start: time(18, 0, 0, 0),
        end: time(19, 0, 0, 0),
        part: DayPart::Dinner,
    },
    Period {
        start: time(23, 0, 0, 0),
        end: time(7, 0, 0, 0),
        part: DayPart::Night,
    },
];

/// Milliseconds in a day, so a fast test clock wraps around at midnight.
const DAY_MILLIS: u128 = 24 * 60 * 60 * 1000;

impl DayPart {
    /// The part of the day that `clock_time` falls in.
    pub fn at(clock_time: Time) -> Self {
        SCHEDULE
            .iter()
            .find(|period| period.contains(clock_time))
            .map_or(DayPart::Free, |period| period.part)
    }

    /// Breakfast or dinner.
    pub fn is_meal(self) -> bool {
        matches!(self, DayPart::Breakfast | DayPart::Dinner)
    }

    /// The nap or the night.
    pub fn is_sleep(self) -> bool {
        matches!(self, DayPart::Nap | DayPart::Night)
    }
}

impl Period {
    fn contains(&self, clock_time: Time) -> bool {
        if self.start <= self.end {
            self.start <= clock_time && clock_time < self.end
        } else {
            self.start <= clock_time || clock_time < self.end
        }
    }
}

/// The clock the cat follows: the local time, or a test clock.
pub struct Clock {
    test: Option<TestClock>,
}

/// A clock that starts at a set time when the window opens, and may run faster.
struct TestClock {
    start: Time,
    /// How many clock seconds pass in one real second.
    speed: u32,
    opened: Instant,
}

impl Clock {
    /// The local time, as set on the computer.
    pub fn local() -> Self {
        Self { test: None }
    }

    /// Reads the test settings `STRAY_CLOCK` and `STRAY_CLOCK_SPEED`. Without either one,
    /// this is the local time.
    pub fn from_env(now: Instant) -> Result<Self> {
        let start = read_setting(env::CLOCK)?;
        let speed = read_setting(env::CLOCK_SPEED)?;
        Self::from_settings(start.as_deref(), speed.as_deref(), now)
    }

    /// Makes a clock from the test settings, which come from `from_env`.
    fn from_settings(start: Option<&str>, speed: Option<&str>, now: Instant) -> Result<Self> {
        if start.is_none() && speed.is_none() {
            return Ok(Self::local());
        }
        let start = match start {
            Some(start) => start
                .parse()
                .with_context(|| format!("{} should be a time such as 06:55", env::CLOCK))?,
            None => local_time(),
        };
        let speed = match speed {
            Some(speed) => speed
                .parse()
                .ok()
                .filter(|speed| *speed > 0)
                .with_context(|| {
                    format!("{} should be a whole number above 0", env::CLOCK_SPEED)
                })?,
            None => 1,
        };
        Ok(Self {
            test: Some(TestClock {
                start,
                speed,
                opened: now,
            }),
        })
    }

    /// The time on the clock at `now`.
    pub fn time(&self, now: Instant) -> Time {
        let Some(test) = &self.test else {
            return local_time();
        };
        let elapsed = now.saturating_duration_since(test.opened).as_millis();
        // Only the time of day matters, so whole days are dropped before the number grows.
        let millis = elapsed % DAY_MILLIS * u128::from(test.speed) % DAY_MILLIS;
        let since_start =
            i64::try_from(millis).map_or(SignedDuration::ZERO, SignedDuration::from_millis);
        test.start.wrapping_add(since_start)
    }
}

/// The local time of day, with the computer's time zone.
fn local_time() -> Time {
    jiff::Zoned::now().time()
}

/// Reads a setting that may be missing. A setting that is not text is an error.
fn read_setting(name: &str) -> Result<Option<String>> {
    std::env::var_os(name)
        .map(|value| {
            value
                .into_string()
                .ok()
                .with_context(|| format!("{name} should be plain text"))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn each_time_falls_in_its_part_of_the_day() {
        for (hour, minute, part) in [
            (6, 59, DayPart::Night),
            (7, 0, DayPart::Breakfast),
            (8, 59, DayPart::Breakfast),
            (9, 0, DayPart::Free),
            (13, 30, DayPart::Nap),
            (15, 0, DayPart::Free),
            (18, 0, DayPart::Dinner),
            (19, 0, DayPart::Free),
            (23, 0, DayPart::Night),
            (0, 0, DayPart::Night),
            (3, 0, DayPart::Night),
        ] {
            assert_eq!(
                DayPart::at(time(hour, minute, 0, 0)),
                part,
                "{hour}:{minute}"
            );
        }
    }

    #[test]
    fn meals_and_sleep_are_told_apart() {
        assert!(DayPart::Breakfast.is_meal() && DayPart::Dinner.is_meal());
        assert!(DayPart::Nap.is_sleep() && DayPart::Night.is_sleep());
        assert!(!DayPart::Free.is_meal() && !DayPart::Free.is_sleep());
    }

    #[test]
    fn a_test_clock_starts_at_its_time_and_runs_faster() -> Result<()> {
        let opened = Instant::now();
        let clock = Clock::from_settings(Some("06:55"), Some("60"), opened)?;

        assert_eq!(clock.time(opened), time(6, 55, 0, 0));
        // Five real seconds at 60 times speed are five minutes on the clock.
        let later = clock.time(opened + Duration::from_secs(5));
        assert_eq!(later, time(7, 0, 0, 0));
        assert_eq!(DayPart::at(later), DayPart::Breakfast);
        Ok(())
    }

    #[test]
    fn a_test_clock_wraps_at_midnight() -> Result<()> {
        let opened = Instant::now();
        let clock = Clock::from_settings(Some("23:59"), Some("60"), opened)?;

        assert_eq!(
            clock.time(opened + Duration::from_secs(2)),
            time(0, 1, 0, 0)
        );
        // Days later the clock still keeps the right time of day.
        let days = Duration::from_secs(10 * 24 * 60);
        assert_eq!(clock.time(opened + days), time(23, 59, 0, 0));
        Ok(())
    }

    #[test]
    fn bad_test_settings_are_errors() {
        let opened = Instant::now();
        for (start, speed) in [
            (Some("breakfast"), None),
            (Some("25:00"), None),
            (None, Some("0")),
            (None, Some("-5")),
            (None, Some("fast")),
        ] {
            assert!(
                Clock::from_settings(start, speed, opened).is_err(),
                "{start:?} {speed:?}"
            );
        }
    }

    #[test]
    fn without_settings_the_clock_is_local() -> Result<()> {
        let clock = Clock::from_settings(None, None, Instant::now())?;
        assert!(clock.test.is_none());
        Ok(())
    }
}
