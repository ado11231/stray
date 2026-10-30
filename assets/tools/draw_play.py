"""Draw the cat playing with its ball: crouch, wiggle, pounce, and bat.

Usage: python3 draw_play.py
It uses the same shapes and head as draw_sit.py, so it is the same cat. The
crouch comes from the standing pose, low in front with the back end up. The
wiggle shakes the back end before the pounce. The pounce leaps forward, and
the player moves the cat along the bar while it plays. The bat swipes a front
paw at the ball. The ball is its own small image, so it can roll on its own.
Frames use the same 309x215 canvas as the other poses, feet on row 204.
"""
import math

from PIL import Image, ImageDraw

import draw_sit as cat
from draw_sit import GROUND, PAW_R, FOOT, SCALE, big, blend, draw_frame, save

# low in front, the back end a little up, looking ahead at the ball. The back legs
# are folded under the body, ready to push: the paw forward, the heel back.
CROUCH = dict(head=(204, 86), haunch=(112, 180, 54, 34, -6), chest=(150, 128, 250, 204),
              leg_top=(226, 178), tail_root=(64, 150), tail_len=50, tail_start=160, tail_bend=-25,
              head_turn=0, paw=(10, GROUND - PAW_R), far_paw=(6, GROUND - PAW_R),
              hind_paw=(140, GROUND - FOOT[1]))
# pushing off: the back legs straighten, the front comes up and reaches, nose up
LAUNCH = dict(head=(212, 58), haunch=(100, 160, 62, 36, 10), chest=(150, 98, 262, 170),
              leg_top=(240, 148), tail_root=(58, 122), tail_len=50, tail_start=145, tail_bend=10,
              head_turn=8, paw=(26, 168), far_paw=(18, 174), hind_paw=(66, GROUND - FOOT[1]), soft_back=True)
# high in the air, stretched out long: front paws reach ahead, back legs trail behind
AIR = dict(head=(214, 22), haunch=(96, 122, 64, 34, 0), chest=(140, 58, 262, 126),
           leg_top=(244, 106), tail_root=(52, 88), tail_len=50, tail_start=138, tail_bend=10,
           head_turn=0, paw=(40, 126), far_paw=(32, 132), hind_paw=(26, 130), soft_back=True)
# front paws down on the ball spot, nose down, the back end still coming down
LAND = dict(head=(208, 88), haunch=(108, 172, 58, 38, -14), chest=(152, 124, 256, 204),
            leg_top=(232, 172), tail_root=(62, 134), tail_len=50, tail_start=140, tail_bend=-30,
            head_turn=-6, paw=(16, GROUND - PAW_R), far_paw=(10, GROUND - PAW_R),
            hind_paw=(86, GROUND - FOOT[1]), soft_back=True)
# the leap, with a frame between each pose so the arc is smooth
POUNCE = [(LAUNCH, AIR, 0), (LAUNCH, AIR, 0.5), (LAUNCH, AIR, 1), (AIR, LAND, 0.5), (AIR, LAND, 1)]

CROUCH_IN = [0.35, 0.7]          # share of the way from STANDING to CROUCH in each crouch frame
WIGGLE_FRAMES = 6
WIGGLE = (3, 2)                  # px the back end shakes, side to side and up and down
WIGGLE_SWAY = 14                 # degrees the tail tip flicks
# batting, the head comes up so the paw can swipe out under the chin
BAT_HEAD = (194, 64)
# the near paw while it bats: lift, reach out, swipe down, back on the floor
BAT_PAWS = [(22, 176), (46, 164), (62, 186), (40, GROUND - PAW_R)]

# the ball, on its own small canvas so it can roll along the bar
BALL_CANVAS = (40, 215)          # same height as the cat, so it scales the same
BALL_R = 15
BALL = (236, 122, 110, 255)      # a soft red, a toy that stands apart from the white cat
STRIPE = (255, 236, 230, 255)
STRIPE_W = 3
BALL_FRAMES = 8                  # one full turn as it rolls


def draw_ball(turn):
    """The ball turned by this share of a full turn. The stripe shows it rolling."""
    im = Image.new("RGBA", big(BALL_CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    mid = (BALL_CANVAS[0] / 2, GROUND - BALL_R)
    cat.dot(draw, mid, BALL_R, BALL)
    a = 2 * math.pi * turn
    for side in (1, -1):
        # a line through the middle, from one edge to the other, that turns with the ball
        dx, dy = side * math.cos(a) * (BALL_R - 2), side * math.sin(a) * (BALL_R - 2)
        draw.line([big(mid), big((mid[0] + dx, mid[1] + dy))], fill=STRIPE, width=STRIPE_W * SCALE)
    return im.resize(BALL_CANVAS, Image.LANCZOS)


head = cat.cut_head()
save("crouch", [draw_frame(head, blend(cat.STANDING, CROUCH, share)) for share in CROUCH_IN])
wiggle = []
for k in range(WIGGLE_FRAMES):
    turn = 2 * math.pi * k / WIGGLE_FRAMES
    x, bottom, *rest = CROUCH["haunch"]
    shake = (x + WIGGLE[0] * math.sin(turn), bottom - WIGGLE[1] * abs(math.sin(turn)), *rest)
    wiggle.append(draw_frame(head, dict(CROUCH, haunch=shake), WIGGLE_SWAY * math.sin(turn)))
save("wiggle", wiggle)
save("pounce", [draw_frame(head, blend(a, b, share)) for a, b, share in POUNCE])
save("bat", [draw_frame(head, dict(CROUCH, head=BAT_HEAD, paw=paw)) for paw in BAT_PAWS])
save("ball", [draw_ball(k / BALL_FRAMES) for k in range(BALL_FRAMES)])
