# Phase 2 Image Check And Basic Bar

Stray checks image support and reserves two bottom rows in supported terminals.
The cat is not drawn yet. Run these checks in kitty and VS Code on macOS.

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

The last two rows should remain empty. Scroll back to check that older output
is still available. Resize above and below 10 physical rows, checking `stty size`
each time. At 9 rows the shell should get all 9; at 10 it should get 8.

Keep a partially typed command at the prompt while resizing. Check that the
cursor and text stay together, including when changing the window width.
Exit Stray, then print more lines. The outer shell should use the full height.
Repeat with `kill -TERM <stray-pid>` from a second window to check cleanup.

This is basic row reservation. Full screen programs can still reset the region
or write into the bar; protecting those cases is a later Phase 2 change.

## Unsupported Terminal

Disable images in VS Code, then repeat the commands above. Wait for the prompt
before typing. The two terminal names should now match: Stray runs the shell
directly. A hint appears only on the first unsupported result across windows.
Run it again to check that the hint stays silent. Restore the image setting.

If you type during the check, those keys must still reach the shell. In that
case Stray keeps a plain PTY wrapper even when images are unavailable, so its
terminal name differs from the outer one. No image or reserved rows are expected.

## tmux

Use a separate server to avoid changing existing sessions:

```sh
tmux -L stray-phase2 -f /dev/null new-session
```

Run Stray with passthrough off, then with `tmux set -g allow-passthrough on`.
Record whether the query succeeds, whether any reply text leaks into the prompt,
and whether keys still work. Placeholder drawing comes later in Phase 2.
Exit each test shell and tmux pane when finished.

## Record Results

Record the terminal, shell, image setting, tmux setting if used, and each result
in the roadmap. Automated PTY tests simulate terminal replies; they do not
establish support or visual behavior in a terminal app.
