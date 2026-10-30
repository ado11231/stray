# Phase 2 The Bar

Stray checks image support, reserves two bottom rows in supported terminals, and
draws a sitting cat, its bowl, and a cat tree there. Run these checks in kitty and
VS Code on macOS.

## Supported Terminal

In VS Code, enable `terminal.integrated.enableImages` first.

```sh
cargo build --release --locked
tty
./target/release/stray start --login
tty
printf 'marker: %s\n' "$STRAY_TTY"
```

The second `tty` should differ from the first and match the marker. There should
be no image hint and no reply text at the prompt. Type, paste, resize, and use
Ctrl C as usual. Type `exit` to return to the outer shell.

## Reserved Rows

Run `stty size` before and inside Stray. At the same window size, the inner shell
should report two fewer rows and the same columns. Print enough lines to scroll:

```sh
for i in {1..100}; do printf 'line %s\n' "$i"; done
```

The last two rows should keep only the cat and its things. Scroll back to check
that older output is still available. Resize above and below 10 physical rows, checking `stty size`
each time. At 9 rows the shell should get all 9; at 10 it should get 8.

Keep a partially typed command at the prompt while resizing. Check that the
cursor and text stay together, including when changing the window width.
Exit Stray, then print more lines. The outer shell should use the full height.
Repeat with `kill -TERM <stray-pid>` from a second window to check cleanup.

## The Cat

The cat tree sits at the left of the bar, then the bowl, then the cat. The cat
should be your text color, with eyes in the background color. Start Stray again
after changing to a light theme: the cat should turn dark.

Resize below 10 rows: the cat and its things go away. Grow the window again: they
come back. On `exit`, they are removed and nothing is left in the bottom rows.

## Keeping The Bar Whole

Each of these should leave the cat in place, or bring it back right away:

```sh
clear
reset
printf '\e[r'; seq 100
printf '\e[999;1Hbottom\n'
```

Press Ctrl L at the prompt. Open and quit `vim` and `less`; the cat should stay
below them, since they only see the shell rows. Run a Claude Code session and a
long build. After the `printf '\e[r'` line, the bar must not scroll away. After
the `\e[999;1H` line, the word should print on the last shell row, not in the bar.

Run `kitten icat` on a picture in kitty. Its picture and the cat should not
remove each other.

In VS Code, watch whether the cat flickers or vanishes as you type at the prompt.
Shells clear below the prompt often, and that may erase picture cells there.

## Unsupported Terminal

Disable images in VS Code, then repeat the commands above. Wait for the prompt
before typing. The two terminal names should now match: Stray runs the shell
directly. A hint appears only on the first unsupported result across windows.
Run it again to check that the hint stays silent. Restore the image setting.

If you type during the check, those keys must still reach the shell. In that
case Stray keeps a plain PTY wrapper even when images are unavailable, so its
terminal name differs from the outer one. No image or reserved rows are expected.

## tmux

Drawing inside tmux moved to a later phase. Inside tmux, Stray should keep the
two rows but draw nothing. Use a separate server to avoid changing existing
sessions:

```sh
tmux -L stray-phase2 -f /dev/null new-session
```

Run Stray with passthrough off, then with `tmux set -g allow-passthrough on`.
Check that no reply text leaks into the prompt and that keys still work.
Exit each test shell and tmux pane when finished.

## Record Results

Record the terminal, shell, image setting, tmux setting if used, and each result
in the roadmap. Automated PTY tests simulate terminal replies; they do not
establish support or visual behavior in a terminal app.
