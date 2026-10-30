# Phase 3 The Cat's Day

The cat follows the clock, does resting actions in free time, and reacts to what
happens in the shell. Run these checks in kitty and VS Code on macOS.

## Start Stray

Build once, then start Stray from the repo folder. Inside the Stray shell, load
the prompt mark once, unless your shell startup already runs `stray init`.

```sh
cargo build --release --locked
./target/release/stray start --login
```

```sh
eval "$(~/Documents/rust/stray/target/release/stray init zsh)"
```

Use `bash` in place of `zsh` to check bash. Type `exit` to leave Stray.

## The Clock

Two test settings change the cat's clock. `STRAY_CLOCK` sets the time when the
window opens, and `STRAY_CLOCK_SPEED` makes it run faster. At speed 60, one
minute on the clock passes each second. The speed also shortens the meal and the
wait before a nap on a long run, so at speed 60 a meal lasts 3 seconds.

```sh
STRAY_CLOCK=06:59 STRAY_CLOCK_SPEED=60 ./target/release/stray start --login
```

After a second it is breakfast. The cat gets up, walks to the bowl, turns to
face it, leans in, and eats for a few seconds. Then it sits by the bowl.

```sh
STRAY_CLOCK=12:59 STRAY_CLOCK_SPEED=60 ./target/release/stray start --login
```

After a second it is nap time. The cat lies down at its home spot and sleeps.
About 2 minutes later the nap ends, and it wakes and sits.

```sh
STRAY_CLOCK=17:59 STRAY_CLOCK_SPEED=60 ./target/release/stray start --login
```

The cat eats dinner, sits by the bowl, then at 23:00, about 5 minutes in, walks
home, turns, and sleeps for the night. Use `STRAY_CLOCK_SPEED=600` to reach the
night in about 30 seconds. Open a second window with the same settings during the
night: its cat should already be asleep.

A bad setting, such as `STRAY_CLOCK=breakfast`, should stop Stray from starting
and run the shell directly with one line that names the setting.

## Free Time

```sh
STRAY_CLOCK=10:00 ./target/release/stray start --login
```

Leave the window alone for ten minutes. The cat should sit most of the time, and
walk, play, and run at least once. It stays between its home spot and the right
edge of the window. Make the window wide and narrow and watch it again.

## Reactions

* Type at the prompt: the cat looks up, and looks down about a second after you stop.
* Run `false` or `ls /nope`: the cat flinches.
* Run `sleep 5` and press Ctrl C: no flinch.
* Press Enter on an empty line after a failure: no flinch.
* Open `vim` or `less`: the cat sits still. Typing in them does not make it look up.
* Run `sleep 30`: the cat sits and watches, and starts no actions.
* Run `sleep 150`: after 2 minutes the cat naps where it sits. It wakes when the
  prompt comes back. With `STRAY_CLOCK_SPEED=60`, `sleep 10` is enough: the nap
  comes after 2 seconds.
* Start `claude`, ask it something, and wait: when it finishes and waits for you,
  the cat perks up once.
* During a nap or the night, type and run `false`: the cat sleeps on.

## Record Results

Record the terminal, the shell, and each result in the roadmap. Automated tests
use a test clock and a fake bar width; they do not show how the cat looks or
moves in a terminal app.
