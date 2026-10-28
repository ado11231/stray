"""Draw the sitting cat: the run cat's head on an upright body, with a blink and a tail sway.

Usage: python3 draw_sit.py
The head is cut from run_body.png so it matches every other pose. The body,
front legs and tail are simple round shapes drawn under it. Frames use the
same 309x215 canvas as sleep and wake, feet on row 204.
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
HEAD_AT = (178, 14)              # where the top left of HEAD_BOX lands

# body, in final pixels
HAUNCH = (96, 120, 196, 204)     # the round back end, sitting on the floor
CHEST = (160, 78, 236, 200)      # the upright front
LEG_TOP = (212, 140)
LEG_W = 20
PAW_R = 11
FAR_LEG_DX = -14                 # the far leg stands a little behind the near one

# tail: a thick curve out the back, along the floor, then up at the tip
TAIL_ROOT = (104, 190)
TAIL_LEN = 95
TAIL_W = (20, 16)
TAIL_BEND = 200                  # degrees the tail turns from root to tip
TAIL_START_DEG = 180             # straight back along the floor
TAIL_SWAY = 10                   # degrees the tip sways

FRAMES = 16
BLINK_AT = (10, 11)              # frames with the eyes shut


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


def draw_tail(draw, sway):
    x, y = TAIL_ROOT
    steps = TAIL_LEN * 2
    for k in range(steps + 1):
        t = k / steps
        # most of the bend happens near the tip, so it lies flat first then curls up
        deg = TAIL_START_DEG - (TAIL_BEND + sway) * t ** 2.5
        dot(draw, (x, y), (TAIL_W[0] + (TAIL_W[1] - TAIL_W[0]) * t) / 2, WHITE)
        x += math.cos(math.radians(deg)) / 2
        y -= math.sin(math.radians(deg)) / 2
        y = min(y, GROUND - TAIL_W[1] / 2)


def draw_leg(draw, x, color, edge=None):
    top = (x, LEG_TOP[1])
    paw = (x, GROUND - PAW_R)
    if edge:
        draw.line([big(top), big(paw)], fill=edge, width=(LEG_W + 2) * SCALE)
        dot(draw, paw, PAW_R + 1, edge)
    draw.line([big(top), big(paw)], fill=color, width=LEG_W * SCALE)
    dot(draw, paw, PAW_R, color)


def draw_frame(head, sway):
    im = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    draw_tail(draw, sway)
    draw_leg(draw, LEG_TOP[0] + FAR_LEG_DX, FAR)
    draw.ellipse([v * SCALE for v in HAUNCH], fill=WHITE)
    draw.ellipse([v * SCALE for v in CHEST], fill=WHITE)
    draw_leg(draw, LEG_TOP[0], WHITE, EDGE)
    im.alpha_composite(head, big(HEAD_AT))
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
    frames.append(draw_frame(blink if k in BLINK_AT else head, sway))
save("sit", frames)
