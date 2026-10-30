"""The canvas, the folders, and the small drawing helpers every draw tool shares.

Shapes are given in final pixels and drawn SCALE times bigger, then each frame
shrinks to CANVAS at the end, which smooths the edges.
"""
import math
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
SPRITES = TOOLS.parent / "sprites"

SCALE = 4                # drawing happens this many times bigger, then shrinks
CANVAS = (309, 215)
GROUND = 204             # feet line, same as sleep and wake
WHITE = (255, 255, 255, 255)
OVAL_POINTS = 48         # points around an oval outline


def big(xy):
    return (xy[0] * SCALE, xy[1] * SCALE)


def dot(draw, xy, r, color):
    x, y = big(xy)
    r *= SCALE
    draw.ellipse([x - r, y - r, x + r, y + r], fill=color)


def oval(draw, middle, half, color):
    """An upright oval with these half widths."""
    x, y = big(middle)
    w, h = half[0] * SCALE, half[1] * SCALE
    draw.ellipse([x - w, y - h, x + w, y + h], fill=color)


def oval_points(middle, rx, ry, turn=0, count=OVAL_POINTS):
    """Points around an oval with these half widths, tipped by turn degrees like the head."""
    a = math.radians(turn)
    points = []
    for k in range(count):
        t = 2 * math.pi * k / count
        u, v = rx * math.cos(t), ry * math.sin(t)
        points.append((middle[0] + u * math.cos(a) + v * math.sin(a), middle[1] - u * math.sin(a) + v * math.cos(a)))
    return points


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


def joint(hip, paw, upper, lower):
    """The bend in a leg of two parts, upper and lower long, on the side away from the head."""
    dx, dy = paw[0] - hip[0], paw[1] - hip[1]
    reach = min(math.hypot(dx, dy), upper + lower - 0.01)
    toward = math.atan2(dy, dx)
    bend = math.acos((upper ** 2 + reach ** 2 - lower ** 2) / (2 * upper * reach))
    a = (hip[0] + upper * math.cos(toward + bend), hip[1] + upper * math.sin(toward + bend))
    b = (hip[0] + upper * math.cos(toward - bend), hip[1] + upper * math.sin(toward - bend))
    return a if a[0] < b[0] else b


def save(name, frames):
    """Replace the frames in assets/sprites/<name> with these, named 00.png and up."""
    folder = SPRITES / name
    folder.mkdir(exist_ok=True)
    for old in folder.glob("*.png"):
        old.unlink()
    for i, im in enumerate(frames):
        im.save(folder / f"{i:02d}.png")
