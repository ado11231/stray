"""Draw the sitting cat, the stand up from lying down into the sit, and the sit down from walking.

Usage: python3 draw_sit.py
The head is cut from run_body.png so it matches every other pose. The body,
front legs and tail are simple round shapes drawn under it. The stand up
moves every shape from a low pose, shaped like the last wake frame, to the
sit. The sit down does the same from a standing pose shaped like the walk:
the back end lowers to the floor while the head rises. Played backwards it
gets the cat up to walk. Frames use the same 309x215 canvas as sleep and wake, feet on row 204.
"""
import math
from pathlib import Path

import numpy as np
from scipy import ndimage
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
# head: where the top left of HEAD_BOX lands. haunch: the back end, an oval
# given as middle x, bottom y, half its length, half its width, and how many
# degrees it is tipped up (0 lies flat, 90 stands up).
# chest: the front. leg_top: where the front legs start. tail_root, tail_len,
# tail_start, tail_bend: the tail leaves here, this long, at this angle
# (180 is straight back), turning this many degrees. head_turn: degrees the
# head tips, below 0 nose down.
SIT = dict(head=(178, 14), haunch=(157, 204, 55, 43, 90), chest=(160, 78, 236, 200),
           leg_top=(212, 140), tail_root=(124, 194), tail_len=95, tail_start=180, tail_bend=200, head_turn=0)
# lying down with the head up, close to the last wake frame
LOW = dict(head=(196, 100), haunch=(131, 204, 65, 40, 0), chest=(140, 136, 256, 204),
           leg_top=(232, 186), tail_root=(74, 196), tail_len=30, tail_start=180, tail_bend=20, head_turn=0)
# standing on four legs, close to the walk
STANDING = dict(head=(193, 42), haunch=(102, 170, 62, 38, 0), chest=(104, 86, 252, 158),
                leg_top=(212, 148), tail_root=(58, 108), tail_len=60, tail_start=105, tail_bend=100, head_turn=0)
# leaning over the bowl to eat, the head tipped down
EAT = dict(head=(204, 100), haunch=(150, 204, 52, 42, 75), chest=(166, 118, 262, 204),
           leg_top=(236, 164), tail_root=(118, 196), tail_len=95, tail_start=180, tail_bend=200, head_turn=-10)

LEG_W = 20
PAW_R = 11
FAR_LEG_DX = -14                 # the far leg stands a little behind the near one
TAIL_W = (20, 16)
TAIL_SWAY = 10                   # degrees the tip sways

FRAMES = 16
LEAN = [0.35, 0.7]               # share of the way from SIT to EAT in each lean down frame
EAT_FRAMES = 8
CHEW = 3                         # px the head bobs while it eats

# the bowl, on its own small canvas so it can stay at home while the cat walks off
BOWL_CANVAS = (100, 215)         # same height as the cat, so it scales the same
BOWL_W, BOWL_H = 86, 26          # rim width, and height from the floor to the rim
BOWL_SLANT = 12                  # px the sides come in toward the floor
BOWL_RIM = 7                     # half height of the rim oval
BOWL = (150, 160, 185, 255)      # a soft grey blue, so it stands apart from the white cat
FOOD = (96, 104, 128, 255)
BLINK_AT = (10, 11)              # frames with the eyes shut
STAND = [0.0, 0.3, 0.6, 0.85]    # share of the way from LOW to SIT in each stand frame
SIT_DOWN = [0.0, 0.3, 0.6, 0.85] # share of the way from STANDING to SIT in each sit down frame
# sitting down, the back end drops first and the front rises after
SIT_DOWN_PACE = dict(haunch=0.6, tail_root=0.6, head=1.6, chest=1.6, leg_top=1.6)
HIND_W = (32, 24, 19)            # back leg width at the hip, the hock, and the foot, like the walk
UPPER, LOWER = 35, 33            # the two parts of the back leg
HIND_PAW_X = 104                 # the back paw stays here while the cat sits down or gets up
SMOOTH = 7                       # px; dips in the outline narrower than this get filled
EARS = 40                        # px from the top of HEAD_BOX to the bottom of the ears
BACK_IN = 0.12                   # how far in from each oval's top and bottom the back line runs
FOOT = (12, 8)                   # half width and half height of the back foot


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


def blend(a, b, share, pace=None):
    """A pose part of the way from a to b. pace can give a part its own speed:
    below 1 it moves early, above 1 it moves late."""
    pace = pace or {}
    out = {}
    for key, start in a.items():
        end = b[key]
        part = share ** pace.get(key, 1)
        if isinstance(start, tuple):
            out[key] = tuple(s + (e - s) * part for s, e in zip(start, end))
        else:
            out[key] = start + (end - start) * part
    return out


def draw_tail(draw, pose, sway):
    x, y = pose["tail_root"]
    steps = round(pose["tail_len"] * 2)
    for k in range(steps + 1):
        t = k / steps
        # most of the bend happens near the tip, so it lies flat first then curls up
        deg = pose["tail_start"] - (pose["tail_bend"] + sway) * t ** 2.5
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


def joint(hip, paw):
    """The hock between hip and paw, on the side away from the head, like the walk."""
    dx, dy = paw[0] - hip[0], paw[1] - hip[1]
    reach = min(math.hypot(dx, dy), UPPER + LOWER - 0.01)
    toward = math.atan2(dy, dx)
    bend = math.acos((UPPER ** 2 + reach ** 2 - LOWER ** 2) / (2 * UPPER * reach))
    a = (hip[0] + UPPER * math.cos(toward + bend), hip[1] + UPPER * math.sin(toward + bend))
    b = (hip[0] + UPPER * math.cos(toward - bend), hip[1] + UPPER * math.sin(toward - bend))
    return a if a[0] < b[0] else b


def haunch_reach(haunch):
    """How far the haunch oval reaches out sideways and up and down from its middle."""
    _, _, length, width, tilt = haunch
    a = math.radians(tilt)
    return (math.hypot(length * math.cos(a), width * math.sin(a)),
            math.hypot(length * math.sin(a), width * math.cos(a)))


def haunch_middle(haunch):
    return haunch[0], haunch[1] - haunch_reach(haunch)[1]


def draw_haunch(draw, haunch):
    x, y = haunch_middle(haunch)
    length, width, tilt = haunch[2:]
    a = math.radians(tilt)
    points = []
    for k in range(64):
        t = 2 * math.pi * k / 64
        u, v = length * math.cos(t), width * math.sin(t)
        points.append(big((x + u * math.cos(a) + v * math.sin(a), y - u * math.sin(a) + v * math.cos(a))))
    draw.polygon(points, fill=WHITE)


def draw_hind_leg(draw, haunch):
    """A back leg from the middle of the haunch to a paw that stays put. As the
    haunch comes down the leg folds, the hock pushing out behind, until the
    haunch sits on it and covers it."""
    hip = haunch_middle(haunch)
    paw = (HIND_PAW_X, GROUND - FOOT[1])
    hock = joint(hip, paw)
    taper(draw, hip, hock, HIND_W[0], HIND_W[1])
    taper(draw, hock, paw, HIND_W[1], HIND_W[2])
    x, y = big((paw[0] + FOOT[0] * 0.4, paw[1]))
    draw.ellipse([x - FOOT[0] * SCALE, y - FOOT[1] * SCALE, x + FOOT[0] * SCALE, y + FOOT[1] * SCALE], fill=WHITE)


def draw_back(draw, pose):
    """Fill between the haunch and the chest, so the back is one line and not two balls."""
    hx, hy1 = pose["haunch"][:2]
    hy0 = hy1 - 2 * haunch_reach(pose["haunch"])[1]
    cx0, cy0, cx1, cy1 = pose["chest"]
    cx = (cx0 + cx1) / 2
    # a little in from the top and bottom of each oval, so the edges stay round
    h_in, c_in = (hy1 - hy0) * BACK_IN, (cy1 - cy0) * BACK_IN
    corners = [(hx, hy0 + h_in), (cx, cy0 + c_in), (cx, cy1 - c_in), (hx, hy1 - h_in)]
    draw.polygon([big(c) for c in corners], fill=WHITE)


def solid(layer):
    """The same picture with no soft edge, so laid over white it leaves no dark line.
    The edge still comes out soft once the frame is shrunk."""
    a = np.array(layer)
    a[..., 3] = np.where(a[..., 3] > 127, 255, 0)
    return Image.fromarray(a)


def smooth(layer, ears_y):
    """Fill the dips where two shapes meet, like behind the neck, so the outline
    is one smooth line. Anything narrower than SMOOTH across gets filled in,
    except above ears_y, so the ears stay apart."""
    a = np.array(layer)
    solid = a[..., 3] > 127
    r = SMOOTH * SCALE
    grown = ndimage.distance_transform_edt(~solid) <= r
    closed = ndimage.distance_transform_edt(grown) > r
    closed[:round(ears_y * SCALE)] = False
    fill = closed & (a[..., 3] < 255)
    a[fill] = WHITE
    return Image.fromarray(a)


def turn_head(head, pose):
    """The head tipped by head_turn around its middle, and where its top left goes."""
    x, y = big(pose["head"])
    if not pose["head_turn"]:
        return head, (round(x), round(y))
    turned = head.rotate(pose["head_turn"], resample=Image.BICUBIC, expand=True)
    x -= (turned.width - head.width) / 2
    y -= (turned.height - head.height) / 2
    return turned, (round(x), round(y))


def draw_bowl():
    """The food bowl that stays at the home spot: a rim, and a body that narrows to the floor."""
    im = Image.new("RGBA", big(BOWL_CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    mid = BOWL_CANVAS[0] / 2
    rim_y = GROUND - BOWL_H
    top, bottom = BOWL_W / 2, BOWL_W / 2 - BOWL_SLANT
    draw.polygon([big(c) for c in [(mid - top, rim_y), (mid + top, rim_y),
                                   (mid + bottom, GROUND), (mid - bottom, GROUND)]], fill=BOWL)
    rim = (mid - top, rim_y - BOWL_RIM, mid + top, rim_y + BOWL_RIM)
    draw.ellipse([v * SCALE for v in rim], fill=BOWL)
    food = (mid - top + 5, rim_y - BOWL_RIM + 3, mid + top - 5, rim_y + BOWL_RIM - 2)
    draw.ellipse([v * SCALE for v in food], fill=FOOD)
    return im.resize(BOWL_CANVAS, Image.LANCZOS)


def draw_frame(head, pose, sway=0):
    im = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    leg_x, leg_y = pose["leg_top"]
    draw_tail(draw, pose, sway)
    if pose["haunch"][1] < GROUND:
        draw_hind_leg(draw, pose["haunch"])
    draw_leg(draw, leg_x + FAR_LEG_DX, leg_y, FAR)
    # the body and head go on their own layer, so their outline can be smoothed
    body = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    body_draw = ImageDraw.Draw(body)
    draw_haunch(body_draw, pose["haunch"])
    body_draw.ellipse([v * SCALE for v in pose["chest"]], fill=WHITE)
    draw_back(body_draw, pose)
    head, head_at = turn_head(head, pose)
    body.alpha_composite(head, head_at)
    im.alpha_composite(smooth(body, pose["head"][1] + EARS))
    draw_leg(ImageDraw.Draw(im), leg_x, leg_y, WHITE, EDGE)
    im.alpha_composite(solid(head), head_at)   # the head stays in front of the leg
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
save("sit_down", [draw_frame(head, blend(STANDING, SIT, share, SIT_DOWN_PACE)) for share in SIT_DOWN])
save("lean", [draw_frame(head, blend(SIT, EAT, share)) for share in LEAN])
eat = []
for k in range(EAT_FRAMES):
    turn = 2 * math.pi * k / EAT_FRAMES
    pose = dict(EAT, head=(EAT["head"][0], EAT["head"][1] + CHEW * (0.5 - 0.5 * math.cos(2 * turn))))
    eat.append(draw_frame(head, pose, TAIL_SWAY * math.sin(turn)))
save("eat", eat)
save("bowl", [draw_bowl()])
