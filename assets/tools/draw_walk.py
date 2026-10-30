"""Draw the calm walk and the run: the run cat's body on legs that plant and step.

Usage: python3 draw_walk.py
The cat is one flat white shape, like the other poses. Each leg grows out of
a round thigh or shoulder, tapers down to a small foot, and has a joint that
bends backward. A foot on the floor stays put while the body passes over it,
then lifts and swings forward to its next spot. The walk and the run use the
same drawing with different steps: the walk moves its legs in diagonal pairs,
the run pushes off with both back legs, lands on both front legs, and rocks
the body between. Frames use the same 309x215 canvas as sleep and wake, feet on row 204.
"""
import math

import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage

from face import add_details
from shapes import CANVAS, GROUND, SCALE, TOOLS, WHITE, big, dot, joint, oval, save, taper

# legs, in final pixels
REAR_DROP = 9            # px the back end sits lower than the front, so the rump is not high
BACK_HIP = (88, 146 + REAR_DROP)     # before the body is raised
FRONT_HIP = (200, 148)
UPPER, LOWER = 36, 33    # the two parts of each leg, nearly straight when planted
BACK_W = (32, 24, 19)    # back leg width at the hip, the joint, and the foot
FRONT_W = (27, 21, 18)   # front legs are a little slimmer
THIGH = (28, 24)         # half width and half height of the round back thigh
SHOULDER = (19, 20)      # the same for the front shoulder
FOOT = (12, 8)           # half width and half height of a foot
FAR_DX = -8              # far legs sit a little behind the near ones

# The steps. stride: px a paw travels back while it is on the floor.
# on_floor: share of each step a paw is planted. lift: px a paw rises at the
# top of its swing. phase: when each leg steps, for back near, back far, front
# near, front far. raise: px the body is lifted, so more leg shows under it.
# spread: px the front paws step further forward and the back paws further back. bob: px the body dips as paws land. rock: degrees the body
# tips nose down as the front lands and nose up as the back pushes off.
# tail_root and tail_tip: degrees above straight back. tail_sway: degrees it sways.
# tail_len: px long. The run tail is shorter so it stays inside the frame when it streams back.
# Walk: diagonal pairs step together, so at a small size you always see one
# clear pair of legs forward and one back.
WALK = dict(frames=8, stride=56, on_floor=0.55, lift=8, phase=(0.0, 0.5, 0.5, 0.0),
            raise_=14, spread=0, bob=1.5, rock=0, tail_root=48, tail_tip=125, tail_sway=4, tail_len=84)
# Run: the back legs push off together, then the front legs land together.
RUN = dict(frames=8, stride=100, on_floor=0.32, lift=18, phase=(0.0, 0.08, 0.5, 0.58),
           raise_=24, spread=14, bob=5, rock=5, tail_root=22, tail_tip=50, tail_sway=8, tail_len=50)

# body and tail, from run_body.png
OLD_TAIL = (72, 97)      # the old curled tail is everything left of and above this
TAIL_ROOT = (72, 110)     # in the body before it is raised, a little inside so the tail grows out of the back
TAIL_W = (21, 14)
TAIL_CURVE = 1.8         # above 1, most of the turn happens near the tip
# the back of run_body.png sags between a high rump and the shoulders; everything
# above this line is cut off so the back runs straight. Points are x, y in final pixels.
BACK_LINE = [(56, 99), (120, 103), (190, 108)]
RUMP_ROUND = 6           # px; the rump's top corner is rounded this much after the cut


def distance(gait):
    """How far the cat moves along the bar in one loop, so the planted paws do not slide."""
    return gait["stride"] / gait["on_floor"]


def paw_spot(hip_x, turn, gait):
    """Where a paw is at this point of its step, as x and y in the frame."""
    stride, on_floor = gait["stride"], gait["on_floor"]
    floor_y = GROUND - FOOT[1]
    if turn < on_floor:
        share = turn / on_floor
        return hip_x + stride / 2 - stride * share, floor_y
    share = (turn - on_floor) / (1 - on_floor)
    ease = (1 - math.cos(math.pi * share)) / 2
    return hip_x - stride / 2 + stride * ease, floor_y - gait["lift"] * math.sin(math.pi * share)


def draw_leg(draw, hip, turn, widths, top, gait, spread=0):
    """One leg: a round top that blends into the body, two tapered parts, a foot.
    spread moves where the paw steps, forward or back from under the hip."""
    paw = paw_spot(hip[0] + spread, turn, gait)
    knee = joint(hip, paw, UPPER, LOWER)
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


def straighten_back(body):
    """Cut the rump down to BACK_LINE, then round off the corner the cut leaves."""
    a = np.array(body)
    xs, ys = zip(*BACK_LINE)
    for col in range(round(xs[0] * SCALE), round(xs[-1] * SCALE)):
        a[:round(np.interp(col / SCALE, xs, ys) * SCALE), col, 3] = 0
    # an opening (shrink, then grow back) rounds corners; only the rump end is touched
    solid = a[..., 3] > 127
    rounded = ndimage.binary_opening(solid, iterations=RUMP_ROUND * SCALE)
    rump = np.zeros_like(solid)
    rump[:, :round(xs[1] * SCALE)] = True
    a[solid & ~rounded & rump, 3] = 0
    return Image.fromarray(a)


def draw_tail(draw, sway, gait):
    """A thick curve that rises from the back and curls over at the tip."""
    x, y = TAIL_ROOT
    steps = gait["tail_len"] * 2
    for k in range(steps + 1):
        t = k / steps
        # it leans back from the root and turns up near the tip, a soft curve, not a pole
        deg = gait["tail_root"] + sway + (gait["tail_tip"] - gait["tail_root"]) * t ** TAIL_CURVE
        dot(draw, (x, y), (TAIL_W[0] + (TAIL_W[1] - TAIL_W[0]) * t) / 2, WHITE)
        x -= math.cos(math.radians(deg)) / 2
        y -= math.sin(math.radians(deg)) / 2


def move(im, dy):
    return im.transform(im.size, Image.AFFINE, (1, 0, 0, 0, 1, -dy), resample=Image.BICUBIC)


def draw_frame(body, k, gait):
    turn = k / gait["frames"]
    turns = [(turn + p) % 1 for p in gait["phase"]]
    # the body sinks a little each time paws land, twice per loop
    dip = gait["bob"] * (0.5 - 0.5 * math.cos(4 * math.pi * turn))
    lift = gait["raise_"]
    hips = [(BACK_HIP[0], BACK_HIP[1] - lift + dip), (FRONT_HIP[0], FRONT_HIP[1] - lift + dip)]
    far = [(x + FAR_DX, y) for x, y in hips]
    back, front = -gait["spread"], gait["spread"]

    im = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    draw_leg(draw, far[0], turns[1], BACK_W, THIGH, gait, back)
    draw_leg(draw, far[1], turns[3], FRONT_W, SHOULDER, gait, front)
    draw_leg(draw, hips[0], turns[0], BACK_W, THIGH, gait, back)
    draw_leg(draw, hips[1], turns[2], FRONT_W, SHOULDER, gait, front)
    top = body.copy()
    draw_tail(ImageDraw.Draw(top), gait["tail_sway"] * math.sin(2 * math.pi * turn), gait)
    # tip the body around the front hip so the back end comes down to the back hip,
    # and rock it with the steps
    tip = math.degrees(math.atan2(REAR_DROP, FRONT_HIP[0] - BACK_HIP[0]))
    tip -= gait["rock"] * math.sin(2 * math.pi * turn)
    top = move(top, (dip - lift) * SCALE)
    im.alpha_composite(top.rotate(tip, resample=Image.BICUBIC, center=big(hips[1])))
    return im.resize(CANVAS, Image.LANCZOS)


if __name__ == "__main__":
    body = straighten_back(cut_old_tail(add_details(Image.open(TOOLS / "run_body.png"))))
    for name, gait in (("walk", WALK), ("run", RUN)):
        save(name, [draw_frame(body, k, gait) for k in range(gait["frames"])])
        print(f"{name}: {gait['frames']} frames, moves {distance(gait):.1f} px per loop, "
              f"{distance(gait) / CANVAS[0]:.3f} image widths")
