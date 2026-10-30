"""Draw the small reactions, all from the sit: look up, flinch, perk up, and petted.

Usage: python3 draw_react.py
It uses the same shapes and head as draw_sit.py, so it is the same cat.
Look up: you type at the prompt, the head tips up toward it, holds, and comes back.
Flinch: a command fails, the cat ducks back with its tail puffed up, then settles.
Perk up: an agent starts, the cat sits up tall and its tail goes up.
Petted: eyes shut, it leans its head into the hand and purrs. While it purrs
the head nuzzles up and down, and small purr marks pulse at its throat.
Each "in" set is played forward to react and backward to settle.
Frames use the same 309x215 canvas as the other poses, feet on row 204.
"""
import math

from PIL import Image, ImageDraw

import draw_sit as cat
from draw_sit import SIT, blend, draw_frame
from shapes import CANVAS, SCALE, WHITE, big, save

# the head tips up toward the prompt above
LOOK = dict(SIT, head=(168, 4), head_turn=22)
# ducked back and low, the tail shot up and puffed, the tip hooked back
FLINCH = dict(SIT, head=(160, 34), head_turn=6, chest=(146, 96, 222, 200), leg_top=(198, 150),
              tail_root=(112, 178), tail_len=88, tail_start=110, tail_bend=-50, tail_w=(30, 24))
# sitting up tall, the tail up with a hook at the tip
PERK = dict(SIT, head=(182, 2), chest=(160, 66, 236, 200), leg_top=(212, 132),
            tail_root=(122, 170), tail_len=85, tail_start=100, tail_bend=60)
# leaning the head into the hand
PETTED = dict(SIT, head=(184, 18), head_turn=-12)

LOOK_IN = [0.5, 1.0]             # share of the way from SIT to LOOK in each frame
FLINCH_IN = [0.7, 1.0]           # it ducks fast
PERK_IN = [0.4, 0.75, 1.0]
PETTED_IN = [0.5, 1.0]
PURR_FRAMES = 8
NUZZLE = 4                       # px the head rubs up and down into the hand
NUZZLE_TURN = 5                  # degrees the head tips as it rubs
PURR_SWAY = 6                    # degrees the tail sways, slowly
# purr marks: short curves at the throat, like sound, big enough to read in the bar
PURR_AT = (238, 138)             # middle of the curves, just in front of the throat
PURR_RADII = (20, 32)            # the inner and outer curve
PURR_SPAN = 40                   # degrees above and below straight ahead each curve reaches
PURR_W = 7


def purr_marks(frame, count):
    """Draw the first count purr curves on the frame."""
    marks = Image.new("RGBA", big(CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(marks)
    x, y = big(PURR_AT)
    for r in PURR_RADII[:count]:
        r *= SCALE
        draw.arc([x - r, y - r, x + r, y + r], -PURR_SPAN, PURR_SPAN, fill=WHITE, width=PURR_W * SCALE)
    frame.alpha_composite(marks.resize(CANVAS, Image.LANCZOS))
    return frame


head = cat.cut_head()
shut = cat.shut_eyes(head)
save("look_up", [draw_frame(head, blend(SIT, LOOK, share)) for share in LOOK_IN])
save("flinch", [draw_frame(head, blend(SIT, FLINCH, share)) for share in FLINCH_IN])
save("perk_up", [draw_frame(head, blend(SIT, PERK, share)) for share in PERK_IN])
save("petted_in", [draw_frame(shut, blend(SIT, PETTED, share)) for share in PETTED_IN])
purr = []
for k in range(PURR_FRAMES):
    turn = 2 * math.pi * k / PURR_FRAMES
    x, y = PETTED["head"]
    pose = dict(PETTED, head=(x, y + NUZZLE * math.sin(turn)),
                head_turn=PETTED["head_turn"] + NUZZLE_TURN * math.sin(turn))
    # the marks pulse out: one curve, then two, twice in each loop
    frame = draw_frame(shut, pose, PURR_SWAY * math.sin(turn))
    purr.append(purr_marks(frame, 1 + k % 2))
save("purr", purr)
