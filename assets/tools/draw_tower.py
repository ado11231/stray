"""Draw the cat tree that stands in the bottom left corner, next to the bowl.

Usage: python3 draw_tower.py
A base, a rope wrapped post, a shelf with a toy ball hanging from it on a
string, and a bed on top with one arm, like a lounger. It is only there to
look at: the cat does not climb it or play with it. It is its own image, like
the bowl, on a canvas as tall as the cat's, so it scales the same, standing
on row 204.
"""
from PIL import Image, ImageDraw

from draw_sit import GROUND, SCALE, big, dot, save

CANVAS = (150, 215)
WOOD = (196, 176, 150, 255)      # a soft warm grey for the base, shelf and bed
ROPE = (214, 190, 140, 255)      # the post, rope colored
ROPE_LINE = (176, 150, 104, 255)
CUSHION = (150, 160, 185, 255)   # the bed's cushion, the same grey blue as the bowl
BALL = (236, 122, 110, 255)      # the same soft red as the toy ball in draw_play.py
STRING = (120, 110, 100, 255)

# boxes are left, top, right, bottom, in final pixels
BASE = (6, GROUND - 15, 144, GROUND)
POST = (60, 30, 90, GROUND - 10)
ROPE_STEP = 10                   # px between the rope lines on the post
ROPE_W = 2
SHELF = (12, 106, 138, 120)
BED = (14, 24, 140, 40)          # the bed's floor
BED_ARM = 18                     # px the arm rises at the left end of the bed
BED_ARM_W = 14
CUSHION_BOX = (30, 15, 136, 30)
ROUND = 7                        # px the corners are rounded
STRING_X = 128                   # the ball hangs from the shelf's right end
STRING_W = 2
BALL_AT = (STRING_X, 160)
BALL_R = 10


def box(left, top, right, bottom):
    return [v * SCALE for v in (left, top, right, bottom)]


def draw_tower():
    im = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(im)
    draw.rectangle(box(*POST), fill=ROPE)
    left, top, right, bottom = POST
    for y in range(top + ROPE_STEP, bottom, ROPE_STEP):
        draw.line([big((left, y)), big((right, y))], fill=ROPE_LINE, width=ROPE_W * SCALE)
    draw.rounded_rectangle(box(*BASE), ROUND * SCALE, fill=WOOD)
    draw.line([big((STRING_X, SHELF[3])), big(BALL_AT)], fill=STRING, width=STRING_W * SCALE)
    dot(draw, BALL_AT, BALL_R, BALL)
    draw.rounded_rectangle(box(*SHELF), ROUND * SCALE, fill=WOOD)
    # the bed: a floor with one arm rising at the left end, and a cushion on it
    left, top, right, bottom = BED
    draw.rounded_rectangle(box(*CUSHION_BOX), ROUND * SCALE, fill=CUSHION)
    draw.rounded_rectangle(box(left, top - BED_ARM, left + BED_ARM_W, bottom), ROUND * SCALE, fill=WOOD)
    draw.rounded_rectangle(box(*BED), ROUND * SCALE, fill=WOOD)
    return im.resize(CANVAS, Image.LANCZOS)


save("tower", [draw_tower()])
