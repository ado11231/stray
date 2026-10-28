"""Play the Stray sprites in kitty the way Stray will: one image, replaced each frame.

Usage: python3 play_in_kitty.py [rows]
The cat sleeps after a > until you press any key. It wakes with a start,
scrambles in place with its paws slipping, bolts off the right edge, then
runs in from the left along the bottom and skids to a stop at its spot.
Press q while it sleeps to quit, or any key at the end.
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

SPRITES = Path("/Users/adnanalagic/Documents/rust/stray/assets/sprites")
ROWS = int(sys.argv[1]) if len(sys.argv) > 1 else 3
IMAGE_ID = 1
SLEEP_MS, WAKE_MS, SCRAMBLE_MS, RUN_MS, STOP_MS = 133, 70, 30, 35, 90
WAKE_UNTIL = 4           # wake frames to show, the hop is last, then it scrambles
SCRAMBLE_LOOPS = 3       # times the paws spin in place before it gets a grip
STRIDE = 1.6             # one run loop covers this many image widths, a scared dash
SPEED_UP = 8             # frames to reach full speed out of the scramble
AWAY_MS = 500            # time out of sight before it comes back
HOME = 0.25              # its spot in the bottom row, as a share of the width
BRAKE = 1.2              # starts slowing this many image widths before its spot
SLOWEST = 0.35           # slowest step while braking, as a share of full speed
SLIDE = [0.6, 0.3, 0.1]  # share of the last bit of distance slid in each stop frame
CHUNK = 4096


IMG_W, IMG_H = struct.unpack(">II", (SPRITES / "run/00.png").read_bytes()[16:24])


def frames(name):
    return [p.read_bytes() for p in sorted((SPRITES / name).glob("*.png"))]


def term_size():
    rows, cols, xpx, ypx = struct.unpack("HHHH", fcntl.ioctl(1, termios.TIOCGWINSZ, b"\0" * 8))
    return rows, cols, xpx / cols, ypx / rows


def show(png, col, row, x_off=0, y_off=0, cut=0):
    """Send one PNG frame at a cell, replacing the last one. Only r= is given,
    so kitty works out the width and the cat keeps its shape. cut hides that
    many image pixels on the left, for a cat still coming in past the edge."""
    data = base64.standard_b64encode(png)
    out = [f"\0337\033[{row + 1};{col + 1}H"]
    keys = f"a=T,f=100,i={IMAGE_ID},q=2,C=1,r={ROWS},X={x_off},Y={y_off}"
    if cut:
        keys += f",x={cut},w={IMG_W - cut}"
    for start in range(0, len(data), CHUNK):
        part = data[start:start + CHUNK]
        more = 1 if start + CHUNK < len(data) else 0
        head = f"{keys},m={more}" if start == 0 else f"m={more}"
        out.append(f"\033_G{head};{part.decode()}\033\\")
    out.append("\0338")
    sys.stdout.write("".join(out))
    sys.stdout.flush()


def key_pressed(timeout):
    ready, _, _ = select.select([sys.stdin], [], [], timeout)
    return sys.stdin.read(1) if ready else None


def main():
    sleep, wake, scramble = frames("sleep"), frames("wake"), frames("scramble")
    run, stop = frames("run"), frames("stop")
    rows, cols, cell_w, cell_h = term_size()
    top = max(rows // 3, 1)
    sys.stdout.write("\033[2J\033[?25l")
    sys.stdout.write(f"\033[1;1HStray test, {ROWS} rows. Press any key to wake the cat, q to quit.")
    sys.stdout.write(f"\033[{top + ROWS};1H>")
    sys.stdout.flush()

    # positions in pixels, so the cat can move less than a cell per frame
    img_h = ROWS * cell_h
    img_w = img_h * IMG_W / IMG_H
    full = STRIDE * img_w / len(run)
    width = cols * cell_w

    def put(png, x, y, ms):
        cut = round(-x * IMG_W / img_w) if x < 0 else 0
        if cut >= IMG_W:   # still fully out of sight
            time.sleep(ms / 1000)
            return
        x = max(x, 0)
        col, row = int(x // cell_w), int(y // cell_h)
        show(png, col, row, int(x - col * cell_w), int(y - row * cell_h), cut)
        time.sleep(ms / 1000)

    i = 0
    while True:
        show(sleep[i % len(sleep)], 2, top)
        key = key_pressed(SLEEP_MS / 1000)
        if key == "q":
            return
        if key:
            break
        i += 1

    for f in wake[:WAKE_UNTIL]:
        show(f, 2, top)
        time.sleep(WAKE_MS / 1000)

    # paws spin in place on the slippery floor
    x, y = 2 * cell_w, top * cell_h
    for k in range(SCRAMBLE_LOOPS * len(scramble)):
        put(scramble[k % len(scramble)], x, y, SCRAMBLE_MS)

    # gets a grip and speeds up, then gone off the right edge
    i = 0
    while x < width:
        put(run[i % len(run)], x, y, RUN_MS)
        x += full * min((i + 1) / SPEED_UP, 1)
        i += 1
    show_nothing()
    time.sleep(AWAY_MS / 1000)

    # back in from the left along the bottom, braking near its spot
    home = HOME * width
    x, y = -img_w, (rows - ROWS) * cell_h
    brake = BRAKE * img_w
    slide = full * SLOWEST * 2
    while x < home - slide:
        left = home - x
        share = max(min(left / brake, 1), SLOWEST)
        put(run[i % len(run)], x, y, RUN_MS / share)
        x += full * share
        i += 1

    # front paws brace and it skids the last bit
    left = home - x
    for f, part in zip(stop, SLIDE):
        put(f, x, y, STOP_MS)
        x += left * part
    while not key_pressed(1):
        pass


def show_nothing():
    sys.stdout.write(f"\033_Ga=d,d=I,i={IMAGE_ID},q=2\033\\")
    sys.stdout.flush()


old = termios.tcgetattr(sys.stdin)
try:
    tty.setcbreak(sys.stdin)
    main()
finally:
    termios.tcsetattr(sys.stdin, termios.TCSADRAIN, old)
    show_nothing()
    sys.stdout.write("\033[?25h\033[2J\033[H")
    sys.stdout.flush()
