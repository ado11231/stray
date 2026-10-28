"""Draw the sitting cat, and the stand up from lying down into the sit.

Usage: python3 draw_sit.py
The head is cut from run_body.png so it matches every other pose. The body,
front legs and tail are simple round shapes drawn under it. The stand up
moves every shape from a low pose, shaped like the last wake frame, to the
sit. Frames use the same 309x215 canvas as sleep and wake, feet on row 204.
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
FAR = (228, 230, 237, 255)   # the far front leg, a little grey so it reads as behind
EDGE = (205, 208, 218, 255)  # thin line that keeps the near leg apart from the body

# head, cut from run_body.png, in final pixels
HEAD_BOX = (192, 60, 285, 165)   # ears, face and chin
HEAD_KEEP = (240, 116, 40, 48)   # keep only what is inside this oval: middle x, y, half width, half height
EYES = [(233, 115, 243, 132), (256, 116, 266, 133)]   # eye boxes in run_body.png, final pixels

# A pose places every shape, in final pixels:
# head: where the top left of HEAD_BOX lands. haunch: the round back end.
# chest: the front. leg_top: where the front legs start. tail_root, tail_len,
# tail_bend: the tail leaves here, this long, turning this many degrees.
SIT = dict(head=(178, 14), haunch=(96, 120, 196, 204), chest=(160, 78, 236, 200),
           leg_top=(212, 140), tail_root=(104, 190), tail_len=95, tail_bend=200)
# lying down with the head up, close to the last wake frame
LOW = dict(head=(196, 100), haunch=(66, 124, 196, 204), chest=(140, 136, 256, 204),
           leg_top=(232, 186), tail_root=(74, 196), tail_len=30, tail_bend=20)

LEG_W = 20
PAW_R = 11
FAR_LEG_DX = -14                 # the far leg stands a little behind the near one
TAIL_W = (20, 16)
TAIL_START_DEG = 180             # straight back along the floor
TAIL_SWAY = 10                   # degrees the tip sways

FRAMES = 16
BLINK_AT = (10, 11)              # frames with the eyes shut
STAND = [0.0, 0.3, 0.6, 0.85]    # share of the way from LOW to SIT in each stand frame


def big(xy):
    return (xy[0] * SCALE, xy[1] * SCALE)


def dot(draw, xy, r, color):
    x, y = big(xy)
    r *= SCALE
    draw.ellipse([x - r, y - r, x + r, y + r], fill=color)


def cut_head():
    """The run cat's head, with the bit of back behind it trimmed off."""
    body = Image.open(TOOLS / "run_body.png").convert("RGBA")
    head = np.array(body.crop([v * SCALE for v in HEAD_BOX]))
    h, w = head.shape[:2]
    ys, xs = np.mgrid[0:h, 0:w]
    cx, cy = [(v - o) * SCALE for v, o in zip(HEAD_KEEP[:2], HEAD_BOX[:2])]
    rx, ry = HEAD_KEEP[2] * SCALE, HEAD_KEEP[3] * SCALE
    inside = ((xs - cx) / rx) ** 2 + ((ys - cy) / ry) ** 2 <= 1
    head[~inside, 3] = 0
    return Image.fromarray(head)


def shut_eyes(head):
    """Paint the eyes white, then a small dark curve low in each, like the sleep cat."""
    out = head.copy()
    draw = ImageDraw.Draw(out)
    for x0, y0, x1, y1 in EYES:
        box = [(x - HEAD_BOX[0]) * SCALE for x in (x0, x1)], [(y - HEAD_BOX[1]) * SCALE for y in (y0, y1)]
        (left, right), (top, bottom) = box
        draw.ellipse([left - SCALE, top - SCALE, right + SCALE, bottom + SCALE], fill=WHITE)
        middle = (top + bottom) / 2
        draw.arc([left, middle - (bottom - top) / 4, right, bottom], 20, 160,
                 fill=(0, 0, 0, 255), width=SCALE * 5)
    return out


def blend(a, b, share):
    """A pose part of the way from a to b, with every number moved the same share."""
    out = {}
    for key, start in a.items():
        end = b[key]
        if isinstance(start, tuple):
            out[key] = tuple(s + (e - s) * share for s, e in zip(start, end))
        else:
            out[key] = start + (end - start) * share
    return out


def draw_tail(draw, pose, sway):
    x, y = pose["tail_root"]
    steps = round(pose["tail_len"] * 2)
    for k in range(steps + 1):
        t = k / steps
        # most of the bend happens near the tip, so it lies flat first then curls up
        deg = TAIL_START_DEG - (pose["tail_bend"] + sway) * t ** 2.5
        dot(draw, (x, y), (TAIL_W[0] + (TAIL_W[1] - TAIL_W[0]) * t) / 2, WHITE)
        x += math.cos(math.radians(deg)) / 2
        y -= math.sin(math.radians(deg)) / 2
        y = min(y, GROUND - TAIL_W[1] / 2)


def draw_leg(draw, x, top_y, color, edge=None):
    top = (x, top_y)
    paw = (x, GROUND - PAW_R)
    if edge:
        draw.line([big(top), big(paw)], fill=edge, width=(LEG_W + 2) * SCALE)
        dot(draw, paw, PAW_R + 1, edge)
    draw.line([big(top), big(paw)], fill=color, width=LEG_W * SCALE)
    dot(draw, paw, PAW_R, color)


def draw_frame(head, pose, sway=0):
    im = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    leg_x, leg_y = pose["leg_top"]
    draw_tail(draw, pose, sway)
    draw_leg(draw, leg_x + FAR_LEG_DX, leg_y, FAR)
    draw.ellipse([v * SCALE for v in pose["haunch"]], fill=WHITE)
    draw.ellipse([v * SCALE for v in pose["chest"]], fill=WHITE)
    draw_leg(draw, leg_x, leg_y, WHITE, EDGE)
    im.alpha_composite(head, tuple(round(v) for v in big(pose["head"])))
    return im.resize(CANVAS, Image.LANCZOS)


def save(name, frames):
    folder = SPRITES / name
    folder.mkdir(exist_ok=True)
    for old in folder.glob("*.png"):
        old.unlink()
    for i, im in enumerate(frames):
        im.save(folder / f"{i:02d}.png")


head = cut_head()
blink = shut_eyes(head)
frames = []
for k in range(FRAMES):
    sway = TAIL_SWAY * math.sin(2 * math.pi * k / FRAMES)
    frames.append(draw_frame(blink if k in BLINK_AT else head, SIT, sway))
save("sit", frames)
save("stand", [draw_frame(head, blend(LOW, SIT, share)) for share in STAND])
