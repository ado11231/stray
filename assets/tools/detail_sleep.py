"""Give the saved sleep and wake frames the same face as the drawn poses: no nose, and a line inside each ear.

Usage: python3 detail_sleep.py
The sources for sleep and wake are not in the repo (see build_sprites.py), so
this edits the saved frames in place. It finds the nose and the ear tips in each
frame, because the head moves while the cat wakes. It has already been run on
the saved frames, so do not run it again: the nose would be left alone, but the
ear lines would be drawn again on the same spots, darkening their soft edges.
"""
import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage

from face import EAR_LINE, EAR_LINE_W
from shapes import SCALE, SPRITES

DARK = 80                # anything darker than this on the cat is an eye or the nose
NOSE_MARGIN = 2          # px painted around the nose, so its soft edge goes too
HEAD = (200, 100, 300, 200)   # where to look for the ears: left, top, right, bottom
EAR_GAP = 15             # px the two ear tips are at least apart
# where each line runs, from its ear tip, the same as face.EAR_LINES on the run cat
EAR_LINE_FROM_TIP = [((2, 9), (4.5, 20)), ((-1, 8.5), (0.5, 18.5))]


def remove_nose(a):
    """Paint the nose white. It is the smallest dark part, below the two eyes."""
    dark = (a[..., 3] > 127) & (a[..., :3].mean(-1) < DARK)
    labels, count = ndimage.label(dark)
    if count < 3:
        return
    sizes = ndimage.sum(dark, labels, range(1, count + 1))
    nose = labels == np.argmin(sizes) + 1
    nose = ndimage.binary_dilation(nose, iterations=NOSE_MARGIN) & (a[..., 3] > 0)
    a[nose, :3] = 255


def ear_tips(a):
    """The top point of each ear, left one first."""
    left, top, right, bottom = HEAD
    # only the cat itself, so a floating Z is never taken for an ear
    labels, count = ndimage.label(a[..., 3] > 127)
    sizes = ndimage.sum(labels > 0, labels, range(1, count + 1))
    cat = labels == np.argmax(sizes) + 1
    solid = cat[top:bottom, left:right]
    tops = np.where(solid.any(0), solid.argmax(0), bottom - top)
    first = int(np.argmin(tops))
    # the other ear is the highest point far enough from the first
    far = np.abs(np.arange(len(tops)) - first) >= EAR_GAP
    second = int(np.argmin(np.where(far, tops, bottom - top)))
    tips = sorted([first, second])
    return [(left + x, top + tops[x]) for x in tips]


def add_ear_lines(im):
    """Draw the ear lines bigger, shrink them for soft edges, and keep them on the cat."""
    lines = Image.new("RGBA", (im.width * SCALE, im.height * SCALE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(lines)
    r = EAR_LINE_W * SCALE / 2
    for (tip_x, tip_y), offsets in zip(ear_tips(np.array(im)), EAR_LINE_FROM_TIP):
        points = [((tip_x + dx) * SCALE, (tip_y + dy) * SCALE) for dx, dy in offsets]
        draw.line(points, fill=EAR_LINE, width=EAR_LINE_W * SCALE)
        for x, y in points:
            draw.ellipse([x - r, y - r, x + r, y + r], fill=EAR_LINE)
    lines = lines.resize(im.size, Image.LANCZOS)
    # only where the cat is, so a line never pokes out past the ear
    alpha = np.minimum(np.array(lines)[..., 3], np.array(im)[..., 3])
    lines.putalpha(Image.fromarray(alpha))
    im.alpha_composite(lines)
    return im


for name in ("sleep", "wake"):
    for path in sorted((SPRITES / name).glob("*.png")):
        a = np.array(Image.open(path).convert("RGBA"))
        remove_nose(a)
        add_ear_lines(Image.fromarray(a)).save(path, optimize=True)
