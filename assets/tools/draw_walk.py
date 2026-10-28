"""Draw the calm walk: the run cat's body on legs that plant and step.

Usage: python3 draw_walk.py
The cat is one flat white shape, like the other poses. Each leg grows out of
a round thigh or shoulder, tapers down to a small foot, and has a joint that
bends backward. A foot on the floor stays put while the body passes over it,
then lifts and swings forward to its next spot. Frames use the same 309x215
canvas as sleep and wake, feet on row 204.
"""
import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

TOOLS = Path(__file__).resolve().parent
SPRITES = TOOLS.parent / "sprites"

SCALE = 4                # drawing happens this many times bigger, then shrinks
CANVAS = (309, 215)
GROUND = 204             # feet line, same as sleep and wake
WHITE = (255, 255, 255, 255)

# legs, in final pixels
RAISE = 14               # px the body is lifted, so more leg shows under it
REAR_DROP = 9            # px the back end sits lower than the front, so the rump is not high
BACK_HIP = (88, 146 - RAISE + REAR_DROP)
FRONT_HIP = (200, 148 - RAISE)
UPPER, LOWER = 36, 33    # the two parts of each leg, nearly straight when planted
BACK_W = (32, 24, 19)    # back leg width at the hip, the joint, and the foot
FRONT_W = (27, 21, 18)   # front legs are a little slimmer
THIGH = (28, 24)         # half width and half height of the round back thigh
SHOULDER = (19, 20)      # the same for the front shoulder
FOOT = (12, 8)           # half width and half height of a foot
FAR_DX = -8              # far legs sit a little behind the near ones

# the step
FRAMES = 8
STRIDE = 56              # px a paw travels back while it is on the floor
ON_FLOOR = 0.55          # share of each step a paw is planted
LIFT = 8                 # px a paw rises at the top of its swing, low so the leg bends softly
# Diagonal pairs step together, so at a small size you always see one clear
# pair of legs forward and one back: back near with front far, then the others.
PHASE = (0.0, 0.5, 0.5, 0.0)     # back near, back far, front near, front far
BOB = 1.5                # px the body dips as each paw lands

# body and tail, from run_body.png
OLD_TAIL = (72, 97)      # the old curled tail is everything left of and above this
TAIL_ROOT = (68, 102)     # in the body before it is raised
TAIL_LEN = 60
TAIL_W = (21, 17)
TAIL_ROOT_DEG = 58       # degrees above straight back where the tail leaves the body
TAIL_TIP_DEG = 140       # the tip curls up and over, like a relaxed cat
TAIL_SWAY = 4            # degrees the tail sways

# How far the cat moves along the bar in one loop, so the planted paws do not slide.
DISTANCE = STRIDE / ON_FLOOR


def big(xy):
    return (xy[0] * SCALE, xy[1] * SCALE)


def dot(draw, xy, r, color):
    x, y = big(xy)
    r *= SCALE
    draw.ellipse([x - r, y - r, x + r, y + r], fill=color)


def oval(draw, middle, half, color):
    x, y = big(middle)
    w, h = half[0] * SCALE, half[1] * SCALE
    draw.ellipse([x - w, y - h, x + w, y + h], fill=color)


def taper(draw, a, b, width_a, width_b):
    """A rounded piece that is width_a wide at a and width_b wide at b."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dy) or 1
    nx, ny = -dy / length, dx / length
    ra, rb = width_a / 2, width_b / 2
    corners = [(a[0] + nx * ra, a[1] + ny * ra), (b[0] + nx * rb, b[1] + ny * rb),
               (b[0] - nx * rb, b[1] - ny * rb), (a[0] - nx * ra, a[1] - ny * ra)]
    draw.polygon([big(c) for c in corners], fill=WHITE)
    dot(draw, a, ra, WHITE)
    dot(draw, b, rb, WHITE)


def paw_spot(hip_x, turn):
    """Where a paw is at this point of its step, as x and y in the frame."""
    floor_y = GROUND - FOOT[1]
    if turn < ON_FLOOR:
        share = turn / ON_FLOOR
        return hip_x + STRIDE / 2 - STRIDE * share, floor_y
    share = (turn - ON_FLOOR) / (1 - ON_FLOOR)
    ease = (1 - math.cos(math.pi * share)) / 2
    return hip_x - STRIDE / 2 + STRIDE * ease, floor_y - LIFT * math.sin(math.pi * share)


def joint(hip, paw):
    """The bend between hip and paw, on the side away from the head."""
    dx, dy = paw[0] - hip[0], paw[1] - hip[1]
    reach = min(math.hypot(dx, dy), UPPER + LOWER - 0.01)
    toward = math.atan2(dy, dx)
    bend = math.acos((UPPER ** 2 + reach ** 2 - LOWER ** 2) / (2 * UPPER * reach))
    a = (hip[0] + UPPER * math.cos(toward + bend), hip[1] + UPPER * math.sin(toward + bend))
    b = (hip[0] + UPPER * math.cos(toward - bend), hip[1] + UPPER * math.sin(toward - bend))
    return a if a[0] < b[0] else b


def draw_leg(draw, hip, turn, widths, top):
    """One leg: a round top that blends into the body, two tapered parts, a foot."""
    paw = paw_spot(hip[0], turn)
    knee = joint(hip, paw)
    # the thigh or shoulder leans a little toward the leg, so it moves with it
    lean = ((knee[0] - hip[0]) * 0.25, (knee[1] - hip[1]) * 0.25)
    oval(draw, (hip[0] + lean[0], hip[1] + lean[1]), top, WHITE)
    taper(draw, hip, knee, widths[0], widths[1])
    taper(draw, knee, paw, widths[1], widths[2])
    oval(draw, (paw[0] + FOOT[0] * 0.4, paw[1]), FOOT, WHITE)   # the foot points forward


def cut_old_tail(body):
    a = np.array(body)
    a[:OLD_TAIL[1] * SCALE, :OLD_TAIL[0] * SCALE] = 0
    return Image.fromarray(a)


def draw_tail(draw, sway):
    """A thick curve that rises from the back and curls over at the tip."""
    x, y = TAIL_ROOT
    steps = TAIL_LEN * 2
    for k in range(steps + 1):
        t = k / steps
        deg = TAIL_ROOT_DEG + sway + (TAIL_TIP_DEG - TAIL_ROOT_DEG) * t
        dot(draw, (x, y), (TAIL_W[0] + (TAIL_W[1] - TAIL_W[0]) * t) / 2, WHITE)
        x -= math.cos(math.radians(deg)) / 2
        y -= math.sin(math.radians(deg)) / 2


def move(im, dy):
    return im.transform(im.size, Image.AFFINE, (1, 0, 0, 0, 1, -dy), resample=Image.BICUBIC)


def draw_frame(body, k):
    turn = k / FRAMES
    turns = [(turn + p) % 1 for p in PHASE]
    # the body sinks a little each time a paw lands, twice per loop
    dip = BOB * (0.5 - 0.5 * math.cos(4 * math.pi * turn))
    hips = [(BACK_HIP[0], BACK_HIP[1] + dip), (FRONT_HIP[0], FRONT_HIP[1] + dip)]
    far = [(x + FAR_DX, y) for x, y in hips]

    im = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    draw_leg(draw, far[0], turns[1], BACK_W, THIGH)
    draw_leg(draw, far[1], turns[3], FRONT_W, SHOULDER)
    draw_leg(draw, hips[0], turns[0], BACK_W, THIGH)
    draw_leg(draw, hips[1], turns[2], FRONT_W, SHOULDER)
    top = body.copy()
    draw_tail(ImageDraw.Draw(top), TAIL_SWAY * math.sin(2 * math.pi * turn))
    # tip the body around the front hip so the back end comes down to the back hip
    tip = math.degrees(math.atan2(REAR_DROP, FRONT_HIP[0] - BACK_HIP[0]))
    top = move(top, (dip - RAISE) * SCALE)
    im.alpha_composite(top.rotate(tip, resample=Image.BICUBIC, center=big(hips[1])))
    return im.resize(CANVAS, Image.LANCZOS)


def save(name, frames):
    folder = SPRITES / name
    folder.mkdir(exist_ok=True)
    for old in folder.glob("*.png"):
        old.unlink()
    for i, im in enumerate(frames):
        im.save(folder / f"{i:02d}.png")


body = cut_old_tail(Image.open(TOOLS / "run_body.png"))
save("walk", [draw_frame(body, k) for k in range(FRAMES)])
print(f"walk: {FRAMES} frames, moves {DISTANCE:.1f} px per loop, {DISTANCE / CANVAS[0]:.3f} image widths")
