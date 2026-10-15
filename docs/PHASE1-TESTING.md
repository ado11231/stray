# Phase 1 Terminal Checks

Run these checks in kitty and in the VS Code terminal on macOS.
Record each result in the roadmap with the terminal name and date.
Automated PTY tests do not prove that a terminal window looks or feels right.

## Build And Start

From the repository:

```sh
cargo build --release --locked
./target/release/stray start --login
```

This starts a nested shell for testing. Type `exit` to return to the outer shell.
Use `--login` when testing login startup files such as `~/.zprofile`.
The wrapper has no cat or reserved rows yet.

Check the marker inside the wrapped shell:

```sh
printf 'marker: %s\n' "$STRAY_TTY"
tty
```

Both lines should name the same terminal. A normal nested `zsh` or `bash`
should keep that terminal and should not start another wrapper.

## Compare Inside And Outside Stray

Run each check in a normal shell and then inside Stray.

| Check | Steps | Expected result |
| --- | --- | --- |
| Typing | Type, paste, use arrows, and search command history. | Same editing and responsiveness. |
| Colors | Run `printf '\033[31mred\033[0m normal\n'`. | Red text followed by normal text. |
| Ctrl C | Run `sleep 30`, press Ctrl C, then run `echo $?`. | Prompt returns promptly; status is 130. |
| vim | Run `vim -Nu NONE -n -i NONE`, type text, resize, then `:q!`. | Editor redraws and the prompt returns normally. |
| Claude Code | Run `claude`, use it normally, then quit. | Input, redraws, and return to the shell behave normally. |
| Resizing | Resize ten times while idle and while vim is open. | Prompt and full screen layout follow the window. |
| Login setup | Compare `printf '%s\n' "$PATH"` and a setting from `.zprofile`. | Expected profile entries and settings remain available. |
| SSH | Connect to a host you normally use, type, resize, and exit. | Remote shell and return to the local prompt behave normally. |

For tmux, use a separate test server so existing sessions are unaffected:

```sh
tmux -L stray-phase1 -f /dev/null new-session
```

Create a pane, type, resize, and leave each pane with `exit`.
Inside tmux started from Stray, `STRAY_TTY` should still name the outer
Stray terminal. The pane's `tty` should differ from that marker.

## New Windows

These checks require Stray's startup integration to be installed and the
`stray` command to be on `PATH`. The automated tests use isolated startup
files; they do not establish behavior with your personal shell setup.

If needed, run `cargo install --path . --locked`, then `stray install`.
This installs the binary and adds a startup line to your shell files.
Open a new terminal afterward. `stray uninstall` removes the startup line.

From an installed Stray shell:

1. Run `code --new-window .` and open a terminal in the new VS Code window.
2. Launch a new kitty window from the same shell, using `kitty` if it is on
   `PATH`, or `/Applications/kitty.app/Contents/MacOS/kitty` on macOS.
3. In each new terminal, compare `STRAY_TTY` with `tty`.

Each new terminal should have a matching marker of its own, different from
the source shell's marker. Starting another shell in the same terminal
should keep its current marker.

## Output Speed

Prepare one file outside Stray, then use that same path inside Stray:

```sh
stray_output_file=$(mktemp -t stray-output)
export stray_output_file
awk 'BEGIN { for (i = 0; i < 50000; i++) print "Stray terminal output check" }' > "$stray_output_file"
printf 'Test file: %s\n' "$stray_output_file"
time cat "$stray_output_file"
```

Run `time cat` three times outside and three times inside Stray. Keep the
terminal size and settings unchanged. Record the times and whether drawing
or scrolling looks slower. Remove the temporary file when finished.

## Record Results

For each terminal, report which checks passed, failed, or were not run.
Include the shell, any error text, and the output timings. Leave Phase 1
open until its required checks pass in both terminal apps.
