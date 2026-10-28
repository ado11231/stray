"""Play the Stray sprites in kitty the way Stray will: one image, replaced each frame.

Usage: python3 play_in_kitty.py [rows]
The cat lives in the bottom rows, like the bar. It sleeps until you press
any key, wakes, stands up and sits. Then it gets up, strolls out along the
bar, sits down a while, and walks back home to the corner, over and over.
Press q at any time to quit.
"""
import base64
import fcntl
import io
import select
import struct
import sys
import termios
import time
import tty
from pathlib import Path

from PIL import Image

SPRITES = Path(__file__).resolve().parent.parent / "sprites"
ROWS = int(sys.argv[1]) if len(sys.argv) > 1 else 3
IMAGE_ID = 1
SLEEP_MS, WAKE_MS, STAND_MS, SIT_MS, WALK_MS, SIT_DOWN_MS = 133, 70, 90, 120, 110, 90
HOME_X = 0               # its spot in the bar: the bottom left corner
AWAY = 0.45              # where it strolls to, as a share of the width
SIT_LOOPS = 2            # times the sit plays before it moves on
WALK_STRIDE = 0.330      # one walk loop covers this many image widths, from draw_walk.py
CHUNK = 4096


def frames(name):
    return [p.read_bytes() for p in sorted((SPRITES / name).glob("*.png"))]


def flipped(pose):
    """The same frames facing left. Stray will do this when it loads its frames."""
    out = []
    for png in pose:
        buf = io.BytesIO()
        Image.open(io.BytesIO(png)).transpose(Image.FLIP_LEFT_RIGHT).save(buf, "PNG")
        out.append(buf.getvalue())
    return out


def term_size():
    rows, cols, xpx, ypx = struct.unpack("HHHH", fcntl.ioctl(1, termios.TIOCGWINSZ, b"\0" * 8))
    return rows, cols, xpx / cols, ypx / rows


def show(png, x, row, cell_w):
    """Send one PNG frame x pixels into a row, replacing the last one. Only r=
    is given, so kitty works out the width and the cat keeps its shape."""
    data = base64.standard_b64encode(png)
    col = int(x // cell_w)
    out = [f"\0337\033[{row + 1};{col + 1}H"]
    keys = f"a=T,f=100,i={IMAGE_ID},q=2,C=1,r={ROWS},X={int(x - col * cell_w)}"
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


def main():
    sleep, wake, stand, sit = frames("sleep"), frames("wake"), frames("stand"), frames("sit")
    walk, sit_down = frames("walk"), frames("sit_down")
    # walk, sit down and sit, facing right (True) or left (False)
    facing = {True: (walk, sit_down, sit),
              False: (flipped(walk), flipped(sit_down), flipped(sit))}
    rows, cols, cell_w, cell_h = term_size()
    row = rows - ROWS
    canvas_w, canvas_h = Image.open(io.BytesIO(sit[0])).size
    img_w = ROWS * cell_h * canvas_w / canvas_h
    step = WALK_STRIDE * img_w / len(walk)
    sys.stdout.write("\033[2J\033[?25l")
    sys.stdout.write(f"\033[1;1HStray test, {ROWS} rows. Any key wakes the cat, q quits.")
    sys.stdout.flush()

    def play(pose, ms, x, times=1, forever=False):
        """Play a pose in place. Returns True if q was pressed."""
        i = 0
        while forever or i < len(pose) * times:
            show(pose[i % len(pose)], x, row, cell_w)
            key = key_pressed(ms / 1000)
            if key == "q":
                return True
            if key and forever:
                return False
            i += 1
        return False

    def stroll(x, to, right):
        """Get up, walk from x to the spot to, and sit down there.
        right is the way it sat. Returns True if q was pressed."""
        if play(facing[right][1][::-1], SIT_DOWN_MS, x):
            return True
        # it turns while standing, then walks the way it goes
        walk, sit_down, sit = facing[to > x]
        i = 0
        while abs(to - x) > step:
            show(walk[i % len(walk)], x, row, cell_w)
            if key_pressed(WALK_MS / 1000) == "q":
                return True
            x += step if to > x else -step
            i += 1
        return play(sit_down, SIT_DOWN_MS, x) or play(sit, SIT_MS, x, SIT_LOOPS)

    home, away = HOME_X, AWAY * cols * cell_w
    if play(sleep, SLEEP_MS, home, forever=True):
        return
    if play(wake, WAKE_MS, home) or play(stand, STAND_MS, home):
        return
    if play(sit, SIT_MS, home, SIT_LOOPS):
        return
    x, right = home, True
    while True:
        to = away if x == home else home
        if stroll(x, to, right):
            return
        x, right = to, to > x


old = termios.tcgetattr(sys.stdin)
try:
    tty.setcbreak(sys.stdin)
    main()
finally:
    termios.tcsetattr(sys.stdin, termios.TCSADRAIN, old)
    show_nothing()
    sys.stdout.write("\033[?25h\033[2J\033[H")
    sys.stdout.flush()
