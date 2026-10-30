"""Small face details for the head in run_body.png: no nose, and a line inside each ear.

draw_sit.py and draw_walk.py both read run_body.png through this, so every
drawn pose gets the same face. Places are in final pixels; run_body.png is
drawn SCALE times bigger.
"""
import numpy as np
from PIL import Image, ImageDraw

SCALE = 4
WHITE = (255, 255, 255)
NOSE_BOX = (242, 132, 258, 147)  # a little bigger than the nose, so its soft edge goes too
# each line runs from near the tip of the ear down toward its middle
EAR_LINES = [((221.5, 79), (224, 90)), ((246.5, 80), (248, 90))]
EAR_LINE_W = 3
EAR_LINE = (150, 155, 170, 255)  # a soft grey, so it reads as a shadow and not an outline


def add_details(body):
    """The run cat with its nose painted out and a line drawn inside each ear."""
    a = np.array(body.convert("RGBA"))
    x0, y0, x1, y1 = (v * SCALE for v in NOSE_BOX)
    a[y0:y1, x0:x1, :3] = WHITE
    out = Image.fromarray(a)
    draw = ImageDraw.Draw(out)
    r = EAR_LINE_W * SCALE / 2
    for start, end in EAR_LINES:
        points = [(x * SCALE, y * SCALE) for x, y in (start, end)]
        draw.line(points, fill=EAR_LINE, width=EAR_LINE_W * SCALE)
        # round ends, so the line does not look cut off
        for x, y in points:
            draw.ellipse([x - r, y - r, x + r, y + r], fill=EAR_LINE)
    return out
