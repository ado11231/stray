# Architecture

* How Stray works inside, and what every source file will do.
* What the cat does: [DESIGN.md](DESIGN.md). Order of work: [ROADMAP.md](ROADMAP.md).
* The image test, shell wrapper, startup image check, bash and zsh startup,
  and install/uninstall are implemented.
* The bar, cat behavior, and pet commands below describe planned work.

## Contents

1. [Overview](#overview)
2. [Key Terms](#key-terms)
3. [The System At A Glance](#the-system-at-a-glance)
4. [How The Code Is Organized](#how-the-code-is-organized)
5. [A Key From Start To Finish](#a-key-from-start-to-finish)
6. [The Bar](#the-bar)
7. [Knowing What Is Happening](#knowing-what-is-happening)
8. [The Cat Brain](#the-cat-brain)
9. [Drawing The Cat](#drawing-the-cat)
10. [The Schedule](#the-schedule)
11. [How Commands Reach The Cat](#how-commands-reach-the-cat)
12. [How Stray Starts](#how-stray-starts)
13. [Where Data Is Stored](#where-data-is-stored)
14. [Safety](#safety)
15. [Hard Parts](#hard-parts)
16. [File Reference](#file-reference)

## Overview

* Stray sits between your terminal app and your shell. Every key and every line of output passes through it.
* It tells the shell the terminal is two rows shorter than it is, and draws the cat in those rows.

1. **Pass everything through.** Keys and output are forwarded as they are.
2. **Keys first.** A key goes to the shell before the cat reacts.
3. **Draw only on change.** If the cat has not moved, Stray draws nothing.

## Key Terms

| Term | Meaning |
| --- | --- |
| **Terminal app** | The window program: kitty, or the terminal in VS Code. |
| **Shell** | The program that reads your commands: bash or zsh. |
| **PTY** | A pretend terminal one program makes for another. |
| **Wrapper** | The running `stray start` program. One per window. |
| **Bar** | The bottom two rows, kept for the cat. |
| **Escape code** | A hidden instruction in terminal output, such as "clear the screen". |
| **Scroll region** | The rows a terminal is allowed to scroll. |
| **Screen model** | Stray's own copy of what is on the screen. |
| **Prompt mark** | A private escape code the shell prints before each prompt. |
| **Foreground program** | The program that owns the terminal right now. |
| **Frame** | One small picture of the cat. |
| **Image protocol** | The escape codes kitty invented for showing pictures. |
| **Image check** | Asking the terminal whether it understands the image protocol. |
| **Schedule** | The times of day the cat eats, naps, and sleeps. |
| **State file** | The cat's name and a few settings, shared by every window. |
| **Batched update** | A program asking the terminal to show many changes at once, so the screen does not flicker. Claude Code does this. |
| **Passthrough** | A tmux setting that lets image codes go through tmux to the terminal app. |
| **Placeholder** | A special text character that stands in for one cell of an image. Used inside tmux. |

## The System At A Glance

```mermaid
flowchart LR
    subgraph Term["Terminal app: kitty or VS Code"]
        direction TB
        Keys(["Your keyboard"])
        Screen["The screen you see"]
    end

    subgraph Wrapper["stray start, one per window"]
        direction TB
        Loop["Main loop"]
        Model["Screen model"]
        Watch["Activity watcher"]
        Clock["Schedule"]
        Brain["Cat brain"]
        Bar["Bar painter"]
        Loop --> Model
        Loop --> Watch
        Watch --> Brain
        Model --> Brain
        Clock --> Brain
        Brain --> Bar
    end

    subgraph Inside["Inside the PTY, two rows shorter"]
        direction TB
        Shell["Your shell"]
        Progs["Programs you run<br/>cargo, vim, Claude Code"]
        Shell --> Progs
    end

    State[("State file<br/>name, quiet,<br/>last treat and pet")]
    Cmds["stray treat, stray pet,<br/>and other commands"]

    Keys -->|keys| Loop
    Loop -->|the same keys| Shell
    Shell -->|output| Loop
    Loop -->|the same output| Screen
    Bar -->|the cat| Screen
    State --> Brain
    Cmds --> State

    style Term fill:none,stroke:#888
    style Wrapper fill:none,stroke:#888
    style Inside fill:none,stroke:#888
```

* No network. Windows share only the state file.

## How The Code Is Organized

* One Rust package, `stray-cat`, that builds one program, `stray`.
* Needs Rust 1.89 or newer.

```mermaid
flowchart TD
    Main["main.rs<br/>reads the command line"]
    Wrap["The wrapper<br/>wrapper, terminal, screen, bar, image"]
    Cat["The cat<br/>cat, frames, foreground, schedule"]
    Store["Saved data<br/>state, config"]
    Cmd["Commands<br/>treat, pet, quiet, name, install"]
    Main --> Wrap
    Main --> Cmd
    Wrap --> Cat
    Wrap --> Store
    Cat --> Store
    Cmd --> Store
```

| Group | Role |
| --- | --- |
| **The wrapper** | Runs the shell in a PTY, passes data, keeps the bar safe, and draws. |
| **The cat** | Decides what the cat does. Knows nothing about terminals, so it tests without one. |
| **Saved data** | Loads and saves the state and settings files. |
| **Commands** | The short commands you type. |

## A Key From Start To Finish

```mermaid
sequenceDiagram
    participant You
    participant Term as Terminal app
    participant Stray as stray start
    participant Shell as Your shell

    You->>Term: press a key
    Term->>Stray: the key
    Stray->>Shell: the same key, sent at once
    Stray->>Stray: note that you are typing
    Shell-->>Stray: output, the letter to show
    Stray->>Stray: update the screen model
    Stray->>Term: the same output
    Stray->>Term: redraw the cat if it moved
    Stray->>Term: put the cursor back
    Term-->>You: the letter appears, the cat reacts
```

* The main loop waits on keys, shell output, and a timer. The timer slows down when the cat is still.

## The Bar

### Reserving The Rows

1. Stray reads the terminal size. Say 40 rows.
2. It creates the PTY with 38 rows.
3. It tells the terminal app to scroll only rows 1 to 38.
4. It draws the bar in rows 39 and 40.

* Row 1 stays row 1, so no positions in the output need changing.
* Text that scrolls off the top still goes into the terminal's history.

### Keeping The Bar Safe

| What Happens | The Damage | What Stray Does |
| --- | --- | --- |
| A program clears the screen | The bar and the cat are wiped. | Draws the bar and places the cat again. |
| A program resets the scroll region | The bar can scroll away. | Swaps the reset for its own region before it reaches the terminal. |
| A program moves the cursor to a row past 38, for example to find the screen size | It would write in the bar. | Changes the move so it stops at row 38. |
| A full screen view opens or closes | The new view has no bar. | Sets the region and draws again. |
| The terminal is reset | Everything is wiped. | Sets the region and draws again. |
| The window is resized | The rows move. | Gives the PTY the new size less two rows, sets the region, and draws again. |
| The window is shorter than 10 rows | No room. | Turns the bar off until the window grows. |

## Knowing What Is Happening

| Signal | How Stray Gets It | What It Tells The Cat |
| --- | --- | --- |
| **Prompt mark** | After a command runs, Stray's own shell prints `ESC ] 7701 ; status BEL` before the prompt. Stray reads it and leaves it in the output. Terminals ignore OSC numbers they do not know. | You are back at the prompt, and whether to flinch. |
| **Foreground program** | Stray asks the PTY which process group is in front (`tcgetpgrp`). When it changes, Stray looks up its name: `proc_name` on macOS, `/proc/<pid>/comm` on Linux. | The shell is waiting, or a program is running, and which one. |
| **Keys** | Stray sees every key. | You are typing, so look up, or how long you have been idle. |
| **Output** | Stray sees all output. | Something is printing, or the screen is quiet. |
| **Full screen flag** | The screen model has it. | A program such as `vim` has the screen. |
| **Clock** | The local time, read once a minute. | Meal time, nap time, night, or free time. |

* Any bytes from the keyboard count as a key. Stray does not work out which key.
* A command that exits with code 130 was stopped with Ctrl C. It does not count as failed, so the cat does not flinch.
* Idle means the shell is in front, with no keys and no output for a set time.
* A coding agent is found by name: `claude` or `codex`.
* `tmux` and `ssh` are found by name too. The cat treats them like any other program.

## The Cat Brain

```mermaid
stateDiagram-v2
    [*] --> Free: a window opens in free time
    [*] --> Sleeping: a window opens at nap or night time
    Free --> Watching: a program runs
    Watching --> Free: back at the prompt
    Free --> Eating: meal time
    Eating --> Free: the meal is done
    Free --> Sleeping: nap or night time, or stray quiet
    Sleeping --> Free: the time ends, or stray come
```

| Mode | Where The Cat Is | What It Does |
| --- | --- | --- |
| **Free** | Anywhere in the bar | Picks a resting action. |
| **Watching** | Where it stopped | Watches, naps after a long run, perks up, or flinches. |
| **Eating** | Its bowl at the home spot | Eats for a few minutes. |
| **Sleeping** | The home spot | Sleeps. Keys do not wake it. |

* The cat never leaves the bar, so its position is just a column in the bar.
* Walking to the home spot for a meal or for sleep is part of the change, not a mode of its own.
* Looking up at a key, a treat, and a pet are short reactions on top of a mode. Then the cat goes back to that mode.
* Each tick, the brain returns a frame, the column to draw it at, and whether anything changed.

### Resting Actions

* In the Free mode, the brain picks an action at random by weight, runs it for a set time, then picks again.

| Action | Starting Weight | Movement |
| --- | --- | --- |
| Sit | 55 | None. |
| Walk | 20 | To a random spot in the bar, slowly. |
| Play | 15 | In place. |
| Run | 10 | Across the bar, fast. |

* The cat rests 5 to 20 seconds between picks.
* For now the weights and times are named constants in `cat.rs`. A settings file comes later.
* A program starting stops new actions. An action already started finishes first.

## Drawing The Cat

* Stray uses two actions of the image protocol: send a frame and show it, and remove it. VS Code supports both.
* It does not place an uploaded frame again by its ID. In VS Code, removing a frame from the screen also deletes the picture, so it cannot be placed again. The image test showed this.

### Once, At Startup

1. Ask the terminal for its text color.
2. Color the frames built into the program, and make a flipped copy of each.
3. Keep every frame in memory, ready to send.

### On Each Change

1. Move to the cat's column in the bar.
2. Send the new frame and show it there, scaled to two rows. Two IDs take turns.
3. Remove the old frame and its picture.
4. Put the cursor back.

* Showing the new frame before removing the old one means there is never a moment with no cat.
* One frame is about 30 KB of text. The cat changes only a few times a second at most, so this is small.
* Every image command tells the terminal not to answer. An answer would arrive as typed keys.
* Each wrapper picks its own random range of frame IDs, and removes only its own frames. Other programs' images and the cats in other tmux panes are safe.
* Stray restores the cursor from the screen model. It does not use the terminal's one "save the cursor" slot, because programs do.

### Only Between Pieces Of Output

* Stray adds its own drawing only at a clean break in the output.
* It never cuts into an escape code or into a character that takes more than one byte.
* During a batched update, it waits until the batch ends, then draws.

### Inside tmux

* tmux does not know about images. A placed image would stay where it is when you switch panes or windows.
* This is planned for a later phase. For now, inside tmux Stray keeps the bar rows but draws nothing.

1. Stray wraps each image code so tmux passes it through to the terminal app.
2. It uploads the frames the same way as outside tmux.
3. To show a frame, it prints placeholders in the bar instead of placing the image at a spot.
4. tmux treats the placeholders as text, so the cat moves and hides with its pane.

* kitty supports placeholders. Other terminals are untested inside tmux.

## The Schedule

1. Each time the cat is settled, sitting or sleeping with nothing planned, it reads the time.
2. It looks the time up in the schedule in `schedule.rs`.
3. If the part of the day changed, the brain plans the way to that mode.

* Nothing is saved. Every window reads the same clock, so every cat keeps the same day.
* A meal lasts a few minutes from the moment it starts, or from when the window opens during it.
* Two test settings change the clock, so a whole day can be checked in a few minutes. `STRAY_CLOCK=06:55` starts the clock at that time when the window opens. `STRAY_CLOCK_SPEED=60` runs it 60 times faster, and makes the meal and the wait before a nap on a long run 60 times shorter.
* Local time with the right time zone comes from the `jiff` crate.

## How Commands Reach The Cat

1. A command such as `stray treat` changes the state file and exits.
2. Every wrapper checks a few times a second whether the file changed.
3. When it has, the wrapper loads it and the cat reacts.

* This is why a command applies in every window.
* `quiet` is saved as on or off.
* `treat` and `pet` are one time events, so each saves the time it was run. Each wrapper sees a newer time than the last one it saw and plays the reaction once.

## How Stray Starts

* `stray install` adds one line to `~/.zshrc` or `~/.bashrc`:

```sh
command -v stray >/dev/null && eval "$(stray init zsh)"
```

```mermaid
flowchart TD
    Start["A shell starts and runs the line"] --> Own{"Is this Stray's own shell?<br/>The mark names this terminal"}
    Own -->|Yes| Hook["Set up the prompt mark<br/>and the cat's name"]
    Hook --> Ready["The prompt appears"]
    Own -->|No| Skip{"Is this an ssh session, or tmux<br/>started from a Stray window?"}
    Skip -->|Yes| Normal["Do nothing<br/>the shell starts as normal"]
    Skip -->|No| Swap["Replace this shell with stray start"]
    Swap --> Check{"Can the terminal<br/>show images?"}
    Check -->|No| Plain["stray start runs your shell directly<br/>with no bar, marked as Stray's own"]
    Check -->|Yes| New["stray start opens a new shell<br/>inside a PTY, marked as Stray's own"]
    New -->|the new shell runs the same line| Start
```

* The mark is an environment variable that holds the name of Stray's PTY, such as `/dev/ttys004`.
* A shell is Stray's own only when the mark matches its own terminal. A copy of the mark that leaked into another program does not count.
* So running `code .` or opening a new kitty window from a Stray shell still gives the new window its own Stray.
* An ssh session is found by the `SSH_CONNECTION` variable that ssh sets.
* tmux started from a Stray window is found by the `TMUX` variable together with a leaked mark.
* If the `stray` program was deleted, the line does nothing and the shell starts as normal.

### The Inner Shell

* The new shell starts as a login shell when the first one was.
* macOS terminals always start login shells, and files such as `~/.zprofile` must still run.
* The new shell gets the same arguments and the same environment, plus the mark.

### The Cat's Name As A Command

* If the cat has a name, `stray init` adds a small shell function with that name:

```sh
mochi() { stray "$@"; }
```

* It is added only if no command with that name exists yet.
* `stray name` checks the name too: one word of letters, numbers, and dashes, and not a command you already have.
* The function is made when a shell starts, so a new name works in new windows.

### The Image Check

1. `stray start` sends a tiny test picture, marked "check only, do not show".
2. It then sends a standard question that every terminal answers.
3. An OK for the picture before that answer means yes.
4. Only the standard answer means no.

* Inside tmux, the test picture is wrapped for passthrough like every other image code.
* If no answer comes back within a short time, that also means no.
* On the first no, Stray prints one hint and creates an `image-hint-shown` marker
  in its data folder. The hint names terminal images and tmux's `allow-passthrough`.
* The check waits up to 500 ms. It removes its replies and keeps other input.
  If keys were read before an unsupported result, a plain PTY wrapper forwards
  them to the shell. With no buffered input, Stray runs the shell directly.
* If the marker cannot be created, Stray skips the hint and still starts the shell.
* Stray never looks at the terminal's name.

## Where Data Is Stored

* Standard folders, found with the `directories` crate. No fixed paths.

| What | File | Holds |
| --- | --- | --- |
| Settings | `config.toml` in the config folder | The schedule, bar height, idle times, action weights, and the test clock shift. |
| State (planned) | `state.json` in the data folder | The cat's name, whether it is quiet, and the time of the last treat and pet. |
| Image hint | `image-hint-shown` in the data folder | Its presence means the hint has been claimed by a window. |

* The state file is written to a new file and swapped in, so a crash cannot leave it half written.

## Safety

* **Fails open.** If anything goes wrong while starting, `stray start` runs your shell directly.
* **Cleans up after a crash.** If Stray crashes after the shell has started, the shell ends with it. Before it exits, Stray puts the terminal back to normal, removes its frames, and prints one line that says what happened.
* **Does not read your work.** Output is used only to follow the cursor, clears, and the prompt mark. None is saved.
* **Does not record anything you do.** The state file holds a name and a few settings.
* **Unknown escape codes pass through.** Features such as VS Code shell integration keep working.
* **No network.**
* **Keeps the exit code.** `stray start` exits with the shell's own code.

## Hard Parts

* None of these is tested yet.

| Problem | Plan |
| --- | --- |
| An escape code can arrive split across two reads. | Hold back an unfinished code until the rest arrives. |
| kitty and VS Code may treat the scroll region differently. | Test the bar in both from its first day. |
| Coding agents redraw the screen often. | Redraw the bar after any output that could have touched it. |
| Tools that run on Node may show the name `node`. | Look at the full command line. |
| A resize can reflow old text into the bar. | Draw the full bar again after every resize. |
| VS Code's image support is new and has gaps. | Send each frame again instead of placing it by ID. The image test showed this works in kitty and VS Code. |
| Swapping frames may flicker. | Show the new frame before removing the old one. No flicker in the image test. |
| A program inside the shell may show its own images. | Random ID ranges, and remove only Stray's frames. |
| Programs can move the cursor into the bar. | Stop every move at the last shell row. |
| Drawing in the middle of a batched update can tear the screen. | Wait for the batch to end. |
| Placed images do not move with tmux panes. | Use placeholders inside tmux. |
| Placeholders may only work in kitty. | Test them in kitty first. Treat other terminals as untested inside tmux. |
| tmux may have passthrough off. | The image check fails and the hint names the setting. |

## File Reference

```
stray/
├── Cargo.toml             the package stray-cat and the program stray
├── src/
│   ├── main.rs            reads the command line and calls the matching command
│   ├── wrapper.rs         stray start: the PTY and the main loop
│   ├── wrapper/           starting the shell, the main loop, and its worker threads
│   ├── escape.rs          named terminal codes, so other files read in plain words
│   ├── env.rs             names of the environment variables Stray reads
│   ├── terminal.rs        the real terminal: raw input, size, and resize
│   ├── screen.rs          the screen model and the checks that keep the bar safe
│   ├── bar.rs             reserves the rows and draws the bar
│   ├── image.rs           the image protocol: check, send, remove, and tmux
│   ├── cat.rs             the cat brain: modes, actions, and reactions
│   ├── cat/               the steps the cat plays, and the plans that build them
│   ├── frames.rs          the cat's frames and its bowl
│   ├── foreground.rs      which program is in front: the shell, an agent, or another
│   ├── schedule.rs        the time of day to meal, nap, night, or free time
│   ├── state.rs           the state file: load and save
│   ├── config.rs          the settings file and its defaults
│   ├── shell.rs           the startup line and the scripts for bash and zsh
│   ├── commands/          one file per command
│   └── bin/
│       └── stray-image-test.rs   Phase 0 only: the image test
└── docs/                  design, architecture, and roadmap
```

| File | Purpose | Uses |
| --- | --- | --- |
| `main.rs` | Reads the command line with `clap`. | every command, `wrapper` |
| `wrapper.rs` | Runs the image check and the shell in a PTY. Owns the main loop. | `terminal`, `screen`, `image`, `bar`, `foreground`, `state` |
| `escape.rs` | Names the terminal codes Stray sends and reads, such as the scroll region reset. | none |
| `env.rs` | Names the environment variables Stray reads, such as `SHELL` and `TMUX`. | none |
| `terminal.rs` | Raw mode, size, and resizes of the real terminal. Restores it on exit. | none |
| `screen.rs` | The screen model. Spots clears, region resets, full screen views, and batched updates. Stops cursor moves into the bar. Removes the prompt mark. | none |
| `bar.rs` | Sets the scroll region, draws the bar, and restores the cursor. | `screen`, `image`, `frames` |
| `image.rs` | The image check. Sends, shows, and removes frames. Inside tmux, wraps codes for passthrough and prints placeholders. | none |
| `cat.rs` | Picks the mode, resting action, reaction, position, and frame. | `foreground`, `schedule`, `frames` |
| `frames.rs` | Holds every frame. Colors and flips them. | none |
| `foreground.rs` | Tells whether the shell, a coding agent, or another program is in front. | none |
| `schedule.rs` | Turns the time of day into meal, nap, night, or free time. Holds the test clock. | none |
| `state.rs` | Loads and saves the state file. | none |
| `config.rs` | Reads the settings file and supplies defaults. | none |
| `shell.rs` | The scripts for `stray init`, the startup line, the name function, and the checks for the mark, ssh, and tmux. | `state` |

### Commands

| File | Command | Uses |
| --- | --- | --- |
| `commands/treat.rs` | `stray treat` | `state` |
| `commands/pet.rs` | `stray pet` | `state` |
| `commands/quiet.rs` | `stray quiet` and `stray come` | `state` |
| `commands/name.rs` | `stray name` | `state` |
| `commands/install.rs` | `stray install` and `stray uninstall` | `shell` |

* `stray start` and `stray init` are hidden helpers. You never type them.

### Libraries

| Crate | Used For |
| --- | --- |
| `portable-pty` | The PTY, the shell inside it, resizing, and the foreground program. |
| `vt100` | The screen model. |
| `crossterm` | Raw mode and the size of the real terminal. |
| `signal-hook` | Noticing when the window is resized. |
| `png` | Reading the frames built into the program. |
| `base64` | Preparing frames to send to the terminal. |
| `clap` | Reading the command line. |
| `serde`, `serde_json`, and `toml` | The state and settings files. |
| `directories` | Finding the config and data folders. |
| `jiff` | The local time, for the schedule. |
| `fastrand` | Picking resting actions and spots at random. |
| `libc` | Asking the PTY which program is in front, and that program's name. |
| `anyhow` | Errors. |

### Tests

* Unit tests sit at the bottom of the file they test.
* `cat.rs`, `schedule.rs`, and `state.rs` need no terminal, so most rules are tested there.
* `screen.rs` is tested by feeding it recorded output.
