"""Draw the scared run, the scramble before it and the skid after, from one legless body.

Usage: python3 draw_run.py
run_body.png is the old run cat with its legs cut off, drawn 4 times bigger.
Legs are drawn as round capsules swung to set angles, so the gallop is easy
to tune here. The near legs are white, the far legs a little grey so they
read as behind. Frames use the same 309x215 canvas as sleep and wake.
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

# legs, in final pixels
BACK_HIP = (82, 146)
FRONT_HIP = (203, 150)
LEG_LEN = 38             # hip to paw middle
LEG_W = 24
PAW_R = 13
NEAR = (255, 255, 255, 255)
FAR = (228, 230, 237, 255)
EDGE = (205, 208, 218, 255)  # thin line that keeps a near leg apart from a far one

FLEX = 6                 # px each end of the body moves when the spine bends
TILT_AT = (140, 140)     # body rocks around this point
OLD_TAIL = (72, 97)      # the old curled tail is everything left of and above this
TAIL_ROOT = (68, 102)
TAIL_LEN = 60
TAIL_W = (21, 17)        # thick at the root, a bit thinner at the tip
TAIL_ROOT_DEG = 28       # degrees above straight back where the tail leaves the body
TAIL_TIP_DEG = 8         # the tip flattens out, streaming behind
DUCK = -3                # degrees the whole cat tips nose down, ducking while it flees

# One row per frame. Leg angles are degrees from straight down, plus is
# toward the head. lift raises a frame with no paw on the ground. flex is -1
# for gathered, 1 for stretched. tilt is degrees, plus is nose up. tail is
# degrees the tail swings, lagging behind the body.
GALLOP = [
    # back near, back far, front near, front far, lift, flex, tilt, tail
    (50, 38, -50, -38, 0, -1.0, 0, 0),     # gathered under the belly
    (25, 40, -15, -38, 0, -0.5, -1, 4),    # back paws land
    (-25, 5, 40, 15, 0, 0.0, 2, 8),        # back paws push off
    (-62, -48, 68, 52, 5, 1.0, 1, 6),      # take off
    (-75, -62, 78, 66, 8, 1.0, 0, 2),      # stretched flat in the air
    (-50, -64, 30, 48, 3, 0.5, -2, -2),    # front paws reach down
    (-10, -30, -5, 15, 0, 0.0, -2, -5),    # front paws land
    (30, 10, -35, -15, 0, -0.5, 0, -3),    # back legs swing forward
]

# Woken with a start: running hard on a slippery floor and going nowhere.
# Each leg swings through a wide arc, middle +- reach, while the body holds
# still. Phase is where in the swing each leg starts, as a share of a turn.
SCRAMBLE_FRAMES = 8
BACK_MIDDLE, FRONT_MIDDLE = -15, 15
REACH = 50
PHASE = (0.0, 0.15, 0.5, 0.65)   # back near, back far, front near, front far
SCRAMBLE_SINK = 2        # px the body sits lower than standing, crouched
SCRAMBLE_TILT = 3        # degrees nose up, leaning back as the paws slip
SCRAMBLE_WOBBLE = 1.5    # degrees the body rocks with the legs
SCRAMBLE_TAIL = 6        # degrees the tail whips


def scramble_table():
    rows = []
    for k in range(SCRAMBLE_FRAMES):
        turn = k / SCRAMBLE_FRAMES
        swing = [math.sin(2 * math.pi * (turn + p)) for p in PHASE]
        rock = math.sin(4 * math.pi * turn)
        rows.append((BACK_MIDDLE + REACH * swing[0], BACK_MIDDLE + REACH * swing[1],
                     FRONT_MIDDLE + REACH * swing[2], FRONT_MIDDLE + REACH * swing[3],
                     0, 0.3 * rock, SCRAMBLE_TILT + SCRAMBLE_WOBBLE * rock,
                     SCRAMBLE_TAIL * swing[0]))
    return rows


# Arriving at its spot: front paws brace, it leans back and slides to a stop.
STOP = [
    (25, 35, 40, 28, 0, -0.5, 6, 8),
    (15, 25, 28, 18, 0, -0.3, 4, 4),
    (5, 12, 8, -4, 0, 0.0, 1, 0),
]

SKID_TOUCH = 5           # a paw this close to the feet line is on the floor
SKID_LEN = 18
SKID_W = 3
SKID = (220, 224, 232, 255)


def big(xy):
    return (xy[0] * SCALE, xy[1] * SCALE)


def dot(draw, xy, r, color):
    x, y = big(xy)
    r *= SCALE
    draw.ellipse([x - r, y - r, x + r, y + r], fill=color)


def paw_at(hip, deg):
    r = math.radians(deg)
    return (hip[0] + LEG_LEN * math.sin(r), hip[1] + LEG_LEN * math.cos(r))


def draw_leg(draw, hip, deg, color, edge=None):
    paw = paw_at(hip, deg)
    if edge:
        draw.line([big(hip), big(paw)], fill=edge, width=(LEG_W + 2) * SCALE)
        dot(draw, hip, LEG_W / 2 + 1, edge)
        dot(draw, paw, PAW_R + 1, edge)
    draw.line([big(hip), big(paw)], fill=color, width=LEG_W * SCALE)
    dot(draw, hip, LEG_W / 2, color)
    dot(draw, paw, PAW_R, color)


def cut_old_tail(body):
    a = np.array(body)
    a[:OLD_TAIL[1] * SCALE, :OLD_TAIL[0] * SCALE] = 0
    return Image.fromarray(a)


def draw_tail(body, swing):
    """A thick curve out behind the cat. The tip swings twice as far as the root."""
    out = body.copy()
    draw = ImageDraw.Draw(out)
    x, y = TAIL_ROOT
    steps = TAIL_LEN * 2
    for k in range(steps + 1):
        t = k / steps
        deg = TAIL_ROOT_DEG + swing + (TAIL_TIP_DEG + swing - TAIL_ROOT_DEG) * t
        dot(draw, (x, y), (TAIL_W[0] + (TAIL_W[1] - TAIL_W[0]) * t) / 2, NEAR)
        x -= math.cos(math.radians(deg)) / 2
        y -= math.sin(math.radians(deg)) / 2
    return out


def bend(body, flex):
    """Pull the back end and the front end apart (stretch) or together (gather)."""
    a = np.array(body)
    shift = flex * FLEX * SCALE
    xs = np.arange(a.shape[1])
    t = np.clip((xs / SCALE - 100) / 80, 0, 1)   # 0 behind the middle, 1 in front
    src = np.clip(np.round(xs + shift * (1 - 2 * t)), 0, a.shape[1] - 1).astype(int)
    return Image.fromarray(a[:, src])


def lowest_row(im):
    return np.nonzero((np.array(im)[..., 3] > 128).any(axis=1))[0].max()


def move(im, dx, dy):
    return im.transform(im.size, Image.AFFINE, (1, 0, -dx, 0, 1, -dy), resample=Image.BICUBIC)


def draw_pose(body, pose):
    back_near, back_far, front_near, front_far, _, flex, tilt, tail = pose
    back = (BACK_HIP[0] - flex * FLEX, BACK_HIP[1])
    front = (FRONT_HIP[0] + flex * FLEX, FRONT_HIP[1])
    im = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    draw_leg(draw, back, back_far, FAR)
    draw_leg(draw, front, front_far, FAR)
    draw_leg(draw, back, back_near, NEAR, EDGE)
    draw_leg(draw, front, front_near, NEAR, EDGE)
    im.alpha_composite(bend(draw_tail(body, tail), flex))
    im = im.rotate(DUCK + tilt, resample=Image.BICUBIC, center=big(TILT_AT))
    return im.resize(CANVAS, Image.LANCZOS)


def draw_frames(body, table):
    """Paws on the ground touch the feet line. Air frames rise from the average."""
    poses = [draw_pose(body, pose) for pose in table]
    drops = [GROUND - lowest_row(im) for im in poses]
    on_ground = [d for d, pose in zip(drops, table) if pose[4] == 0]
    middle = sum(on_ground) / len(on_ground)
    frames = []
    for im, drop, pose in zip(poses, drops, table):
        lift = pose[4]
        frames.append(move(im, 0, drop if lift == 0 else round(middle - lift)))
    return frames


def paws_on_floor(im):
    """Left edge of each paw touching the feet line."""
    a = np.array(im)[GROUND - SKID_TOUCH:GROUND + 1, :, 3] > 128
    cols = a.any(axis=0)
    return [x for x in range(1, len(cols)) if cols[x] and not cols[x - 1]]


def draw_in_place(body, table, sink):
    """Every frame at one height: as low as a cat standing on straight legs, plus sink."""
    standing = draw_pose(body, (0, 0, 0, 0, 0, 0, table[0][6], 0))
    drop = GROUND - lowest_row(standing) + sink
    frames = []
    for pose in table:
        a = np.array(move(draw_pose(body, pose), 0, drop))
        a[GROUND + 1:, :, 3] = 0   # paws press flat on the floor
        frames.append(Image.fromarray(a))
    return frames


def add_skids(frames, count):
    """Short lines on the floor trailing back from the paws, on the first frames."""
    for im in frames[:count]:
        draw = ImageDraw.Draw(im)
        for x in paws_on_floor(im):
            x -= 3
            for dy, length in ((2, SKID_LEN), (8, SKID_LEN * 0.6)):
                y = GROUND - dy
                draw.line([(x - length, y), (x, y)], fill=SKID, width=SKID_W)
    return frames


def save(name, frames):
    folder = SPRITES / name
    folder.mkdir(exist_ok=True)
    for old in folder.glob("*.png"):
        old.unlink()
    for i, im in enumerate(frames):
        im.save(folder / f"{i:02d}.png")


body = cut_old_tail(Image.open(TOOLS / "run_body.png"))
save("run", draw_frames(body, GALLOP))
save("scramble", draw_in_place(body, scramble_table(), SCRAMBLE_SINK))
save("stop", add_skids(draw_frames(body, STOP), 2))
