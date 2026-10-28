"""Play the Stray sprites in kitty the way Stray will: one image, replaced each frame.

Usage: python3 play_in_kitty.py [rows]
The cat lives in the bottom rows, like the bar. It sleeps until you press
any key, wakes, stands up, then sits there blinking and swaying its tail.
Press q at any time to quit.
"""
import base64
import fcntl
import select
import struct
import sys
import termios
import time
import tty
from pathlib import Path

SPRITES = Path(__file__).resolve().parent.parent / "sprites"
ROWS = int(sys.argv[1]) if len(sys.argv) > 1 else 3
IMAGE_ID = 1
SLEEP_MS, WAKE_MS, STAND_MS, SIT_MS = 133, 70, 90, 120
HOME_COL = 0             # its spot in the bar: the bottom left corner
CHUNK = 4096


def frames(name):
    return [p.read_bytes() for p in sorted((SPRITES / name).glob("*.png"))]


def term_size():
    rows, cols, _, _ = struct.unpack("HHHH", fcntl.ioctl(1, termios.TIOCGWINSZ, b"\0" * 8))
    return rows, cols


def show(png, col, row):
    """Send one PNG frame at a cell, replacing the last one. Only r= is given,
    so kitty works out the width and the cat keeps its shape."""
    data = base64.standard_b64encode(png)
    out = [f"\0337\033[{row + 1};{col + 1}H"]
    keys = f"a=T,f=100,i={IMAGE_ID},q=2,C=1,r={ROWS}"
    for start in range(0, len(data), CHUNK):
        part = data[start:start + CHUNK]
        more = 1 if start + CHUNK < len(data) else 0
        head = f"{keys},m={more}" if start == 0 else f"m={more}"
        out.append(f"\033_G{head};{part.decode()}\033\\")
    out.append("\0338")
    sys.stdout.write("".join(out))
    sys.stdout.flush()


def show_nothing():
    sys.stdout.write(f"\033_Ga=d,d=I,i={IMAGE_ID},q=2\033\\")
    sys.stdout.flush()


def key_pressed(timeout):
    ready, _, _ = select.select([sys.stdin], [], [], timeout)
    return sys.stdin.read(1) if ready else None


def play(pose, ms, col, row, loop=True):
    """Play a pose once, or loop it until a key. Returns the key, if any."""
    i = 0
    while loop or i < len(pose):
        show(pose[i % len(pose)], col, row)
        key = key_pressed(ms / 1000)
        if key:
            return key
        i += 1
    return None


def main():
    sleep, wake, stand, sit = frames("sleep"), frames("wake"), frames("stand"), frames("sit")
    rows, _ = term_size()
    col, row = HOME_COL, rows - ROWS
    sys.stdout.write("\033[2J\033[?25l")
    sys.stdout.write(f"\033[1;1HStray test, {ROWS} rows. Any key wakes the cat, q quits.")
    sys.stdout.flush()

    if play(sleep, SLEEP_MS, col, row) == "q":
        return
    if play(wake, WAKE_MS, col, row, loop=False) == "q":
        return
    if play(stand, STAND_MS, col, row, loop=False) == "q":
        return
    while play(sit, SIT_MS, col, row) != "q":
        pass


old = termios.tcgetattr(sys.stdin)
try:
    tty.setcbreak(sys.stdin)
    main()
finally:
    termios.tcsetattr(sys.stdin, termios.TCSADRAIN, old)
    show_nothing()
    sys.stdout.write("\033[?25h\033[2J\033[H")
    sys.stdout.flush()
