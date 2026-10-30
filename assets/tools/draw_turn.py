"""Draw the turn: the cat turning around while it stands, in two frames.

Usage: python3 draw_turn.py
A flat squeeze looks like a card flipping, so the turn has real poses in
between: first a three quarter view, the body shorter and the head coming
round, then the cat facing you. Play the two frames, then the flipped copy
backwards, and the cat ends up facing the other way in the same spot.
It uses the same shapes as draw_sit.py, so it is the same cat.
Frames use the same 309x215 canvas as the other poses, feet on row 204.
"""
from PIL import Image, ImageDraw

import draw_sit as cat
from draw_sit import CANVAS, FAR, FOOT, GROUND, PAW_R, SCALE, STANDING, WHITE, big, draw_frame, save
from face import EAR_LINE, EAR_LINE_W

MIDDLE = 156             # middle of the standing walk frame, so the turn stays in place
INK = (0, 0, 0, 255)

# three quarter: seen at a slant the body is shorter, and the head comes round over the chest
THREE_QUARTER = dict(STANDING, head=(156, 40), haunch=(112, 170, 48, 38, 0), chest=(110, 90, 216, 160),
                     leg_top=(176, 150), tail_root=(72, 110), hind_paw=(108, GROUND - FOOT[1]),
                     far_paw=(12, GROUND - PAW_R))

# facing you, all in px from MIDDLE across and from the top down
FACE = (0, 96, 40, 32)                   # middle x, middle y, half width, half height
EAR = [(-38, 84), (-27, 50), (-8, 68)]   # the left ear; the right one is the same, flipped
EAR_LINE_AT = ((-26.5, 57), (-24, 68))   # the line inside the left ear
EYE = (-16.5, 91.5, -6.5, 108.5)         # the left eye box, the same size as the run cat's eyes
CHEST = (-36, 104, 36, 180)
BACK_END = (-46, 112, 46, 172)           # the back end, wider, seen behind the chest
FRONT_LEG_X = 13
LEG_TOP = 150
# the back legs are further away, so only their lower part peeks out at the sides
BACK_LEG_X = 31
BACK_LEG_TOP = 158      # tucked behind the back end, so the top of the leg never shows
# the tail rises behind the back end, off to one side, the tip curling in
FRONT_TAIL = dict(tail_root=(MIDDLE + 20, 120), tail_len=70, tail_start=60, tail_bend=-40,
                  tail_w=cat.TAIL_W, haunch=(MIDDLE, 160, 46, 30, 0))
EARS_BOTTOM = 70         # the outline smoothing stops above this, so the ears stay apart


def at(x, y):
    return (MIDDLE + x, y)


def both_sides(points):
    """The left side as given, and the right side flipped across the middle."""
    return [points, [(-x, y) for x, y in points]]


def box(x0, y0, x1, y1):
    return [v * SCALE for v in (MIDDLE + x0, y0, MIDDLE + x1, y1)]


def draw_front():
    im = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    cat.draw_tail(draw, draw, FRONT_TAIL, 0)
    for side in (-1, 1):
        x = side * BACK_LEG_X
        cat.draw_leg(draw, at(x, BACK_LEG_TOP), at(x, GROUND - PAW_R), FAR)
    # the body and head on their own layer, so the outline can be smoothed like the other poses
    body = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    body_draw = ImageDraw.Draw(body)
    body_draw.ellipse(box(*BACK_END), fill=WHITE)
    body_draw.ellipse(box(*CHEST), fill=WHITE)
    x, y, rx, ry = FACE
    body_draw.ellipse(box(x - rx, y - ry, x + rx, y + ry), fill=WHITE)
    for ear in both_sides(EAR):
        body_draw.polygon([big(at(*p)) for p in ear], fill=WHITE)
    im.alpha_composite(cat.smooth(body, EARS_BOTTOM))
    for side in (-1, 1):
        x = side * FRONT_LEG_X
        cat.draw_leg(draw, at(x, LEG_TOP), at(x, GROUND - PAW_R), WHITE, cat.EDGE)
    # eyes and ear lines go on last, over the smoothed outline
    x0, y0, x1, y1 = EYE
    for eye in (box(x0, y0, x1, y1), box(-x1, y0, -x0, y1)):
        draw.ellipse(eye, fill=INK)
    r = EAR_LINE_W * SCALE / 2
    for line in both_sides(EAR_LINE_AT):
        points = [big(at(*p)) for p in line]
        draw.line(points, fill=EAR_LINE, width=EAR_LINE_W * SCALE)
        for px, py in points:
            draw.ellipse([px - r, py - r, px + r, py + r], fill=EAR_LINE)
    return im.resize(CANVAS, Image.LANCZOS)


save("turn", [draw_frame(cat.cut_head(), THREE_QUARTER), draw_front()])
