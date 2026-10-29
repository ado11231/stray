"""Play the Stray sprites in kitty the way Stray will: one image, replaced each frame.

Usage: python3 play_in_kitty.py [rows]
The cat lives in the bottom rows, like the bar. It sleeps until you press
any key, wakes, stands up and sits. Then it gets up and goes after its
ball: it crouches, wiggles, pounces and bats it along the bar, a few times.
Then it walks back home to the corner and eats from its bowl, and does it
all again. When the ball gets to the far end, it is put back for the next go.
Press q at any time to quit.
"""
import base64
import fcntl
import io
import math
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
IMAGE_ID, BOWL_ID, BALL_ID = 1, 2, 3
SLEEP_MS, WAKE_MS, STAND_MS, SIT_MS, WALK_MS, SIT_DOWN_MS = 133, 70, 90, 120, 110, 90
LEAN_MS, EAT_MS = 110, 120
CROUCH_MS, WIGGLE_MS, POUNCE_MS, BAT_MS, ROLL_MS = 90, 80, 90, 90, 50
HOME_X = 0               # its spot in the bar: the bottom left corner
SIT_LOOPS = 2            # times the sit plays before it moves on
WALK_STRIDE = 0.330      # one walk loop covers this many image widths, from draw_walk.py
BOWL_X = 301             # where the bowl sits from the cat's left edge, in sprite pixels, past the sleeping cat
LEAN_STEP = (22, 44)     # sprite pixels the cat scoots toward the bowl in each lean frame
EAT_STEP = 64            # and while it eats
EAT_LOOPS = 3
BALL_START = 0.12        # where the ball starts, as a share of the width
BALL_MID, BALL_R = 20, 12    # the ball's middle on its canvas and its size, in sprite pixels, from draw_play.py
BAT_REACH = 288          # how far from the cat's left edge its bat lands, in sprite pixels
POUNCE_STEP = (10, 45, 70)   # sprite pixels the cat has moved in each pounce frame
WIGGLE_LOOPS = 2
PLAY_ROUNDS = 3
ROLL = 1.2               # how far one bat rolls the ball, in image widths
ROLL_FRAMES = 14
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


def show(png, x, row, cell_w, image_id=IMAGE_ID):
    """Send one PNG frame x pixels into a row, replacing the last one with the
    same id. Only r= is given, so kitty works out the width and the cat keeps
    its shape. The bowl and ball go above the cat, so the cat eats behind the rim."""
    data = base64.standard_b64encode(png)
    col = int(x // cell_w)
    out = [f"\0337\033[{row + 1};{col + 1}H"]
    z = 0 if image_id == IMAGE_ID else 1
    keys = f"a=T,f=100,i={image_id},q=2,C=1,r={ROWS},z={z},X={int(x - col * cell_w)}"
    for start in range(0, len(data), CHUNK):
        part = data[start:start + CHUNK]
        more = 1 if start + CHUNK < len(data) else 0
        head = f"{keys},m={more}" if start == 0 else f"m={more}"
        out.append(f"\033_G{head};{part.decode()}\033\\")
    out.append("\0338")
    sys.stdout.write("".join(out))
    sys.stdout.flush()


def show_nothing():
    for image_id in (IMAGE_ID, BOWL_ID, BALL_ID):
        sys.stdout.write(f"\033_Ga=d,d=I,i={image_id},q=2\033\\")
    sys.stdout.flush()


def key_pressed(timeout):
    ready, _, _ = select.select([sys.stdin], [], [], timeout)
    return sys.stdin.read(1) if ready else None


class Quit(Exception):
    """q was pressed."""


def main():
    sleep, wake, stand, sit = frames("sleep"), frames("wake"), frames("stand"), frames("sit")
    walk, sit_down = frames("walk"), frames("sit_down")
    lean, eat, bowl = frames("lean"), frames("eat"), frames("bowl")[0]
    crouch, wiggle, pounce, bat, ball = (frames(n) for n in ("crouch", "wiggle", "pounce", "bat", "ball"))
    # walk, sit down and sit, facing right (True) or left (False)
    facing = {True: (walk, sit_down, sit),
              False: (flipped(walk), flipped(sit_down), flipped(sit))}
    rows, cols, cell_w, cell_h = term_size()
    row = rows - ROWS
    canvas_w, canvas_h = Image.open(io.BytesIO(sit[0])).size
    img_w = ROWS * cell_h * canvas_w / canvas_h
    step = WALK_STRIDE * img_w / len(walk)
    px = img_w / canvas_w    # screen pixels per sprite pixel
    sys.stdout.write("\033[2J\033[?25l")
    sys.stdout.write(f"\033[1;1HStray test, {ROWS} rows. Any key wakes the cat, q quits.")
    sys.stdout.flush()

    def wait(ms):
        if key_pressed(ms / 1000) == "q":
            raise Quit

    def play(pose, ms, x, times=1):
        """Play a pose in place."""
        for i in range(len(pose) * times):
            show(pose[i % len(pose)], x, row, cell_w)
            wait(ms)

    def walk_to(x, to):
        """Walk from x to the spot to, facing the way it goes. Returns where it stops."""
        steps = facing[to > x][0]
        i = 0
        while abs(to - x) > step:
            show(steps[i % len(steps)], x, row, cell_w)
            wait(WALK_MS)
            x += step if to > x else -step
            i += 1
        return to

    def eat_meal(x):
        """Scoot up to the bowl, eat, and scoot back to sit."""
        steps = [x + step_px * px for step_px in LEAN_STEP]
        for png, at in zip(lean, steps):
            play([png], LEAN_MS, at)
        play(eat, EAT_MS, x + EAT_STEP * px, EAT_LOOPS)
        for png, at in zip(lean[::-1], steps[::-1]):
            play([png], LEAN_MS, at)
        play(sit, SIT_MS, x)

    def show_ball(ball_x, turn):
        show(ball[round(turn * len(ball)) % len(ball)], ball_x, row, cell_w, BALL_ID)

    def play_ball(x, ball_x, far):
        """One round: crouch, wiggle, pounce, bat the ball, and watch it roll.
        It will not roll past far. Returns where the cat and the ball end up."""
        play(crouch, CROUCH_MS, x)
        play(wiggle, WIGGLE_MS, x, WIGGLE_LOOPS)
        for png, step_px in zip(pounce, POUNCE_STEP):
            play([png], POUNCE_MS, x + step_px * px)
        x += POUNCE_STEP[-1] * px
        play(bat, BAT_MS, x)
        # the ball rolls off and slows down, while the cat gets up to look
        roll = min(ROLL * img_w, far - ball_x)
        after = [crouch[1], crouch[0], walk[0]]
        for i in range(1, ROLL_FRAMES + 1):
            share = 1 - (1 - i / ROLL_FRAMES) ** 2
            show_ball(ball_x + roll * share, roll * share / (2 * math.pi * BALL_R * px))
            show(after[min(i - 1, len(after) - 1)], x, row, cell_w)
            wait(ROLL_MS)
        return x, ball_x + roll

    def spot_for(ball_x):
        """Where to crouch so the pounce lands the cat right at the ball."""
        return ball_x + BALL_MID * px - (BAT_REACH + POUNCE_STEP[-1]) * px

    home = HOME_X
    start, far = BALL_START * cols * cell_w, cols * cell_w - BALL_MID * 2 * px
    ball_x = start
    show(bowl, home + BOWL_X * px, row, cell_w, BOWL_ID)
    show_ball(ball_x, 0)
    # it sleeps until a key is pressed
    i = 0
    while True:
        show(sleep[i % len(sleep)], home, row, cell_w)
        key = key_pressed(SLEEP_MS / 1000)
        if key == "q":
            raise Quit
        if key:
            break
        i += 1
    play(wake, WAKE_MS, home)
    play(stand, STAND_MS, home)
    play(sit, SIT_MS, home, SIT_LOOPS)
    while True:
        # out to play
        if ball_x >= far:
            ball_x = start
            show_ball(ball_x, 0)
        play(sit_down[::-1], SIT_DOWN_MS, home)
        x = home
        for _ in range(PLAY_ROUNDS):
            if ball_x >= far:
                break
            x = walk_to(x, spot_for(ball_x))
            x, ball_x = play_ball(x, ball_x, far)
        # home to eat, turning to face the bowl before it sits
        x = walk_to(x, home)
        play(sit_down, SIT_DOWN_MS, x)
        play(sit, SIT_MS, x, SIT_LOOPS)
        eat_meal(x)


old = termios.tcgetattr(sys.stdin)
try:
    tty.setcbreak(sys.stdin)
    main()
except Quit:
    pass
finally:
    termios.tcsetattr(sys.stdin, termios.TCSADRAIN, old)
    show_nothing()
    sys.stdout.write("\033[?25h\033[2J\033[H")
    sys.stdout.flush()
