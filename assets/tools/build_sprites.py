"""Build the sleep and wake sprite frames from the smooth v3 animations.

This is kept as the record of how sleep and wake were made. Its source
animations are not in the repo. The old run and leap are still built here,
only to size the shared 309x215 canvas the same way; they are not saved.
The run is drawn by draw_walk.py now.
Every frame goes on one shared canvas, with the cat's feet on the same line,
so a frame can be drawn at a fixed spot without lining anything up.
Work happens in the sleep animation's pixel space, then shrinks at the end.
"""
import glob
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

scratch, out_root = Path(sys.argv[1]), Path(sys.argv[2])
sleep_src = sorted(glob.glob(str(scratch / "f/a_*.png")))
run_src = sorted(glob.glob(str(scratch / "r/r_*.png")))

OUT_SCALE = 0.5          # final frames are half the sleep animation's size
SLEEP_STEP = 4           # keep every 4th of 108 frames -> 27, loops cleanly

# sleep cat, in sleep pixel space
ZZZ_BOX = (424, 0, 512, 120)
EYES = [(357, 239), (415, 239)]
CLOSED_EYE_BOXES = [(339, 230, 376, 247), (399, 230, 433, 248)]
EYE_W = 21               # matches the run cat's eyes once scaled
EYE_GROW = 3             # px of room around closed eye lines
EYE_THICKEN = 3          # filter size that makes the lines about 1 px thicker each side

# zzz, drawn big enough to read at 3 rows tall
Z_COUNT = 3
Z_START = (430, 150)     # just above and right of the ears
Z_END = (486, 26)        # top Z stays inside the image
Z_SMALL = 14
Z_BIG = 34
Z_STROKE = 10
Z_FADE = 0.2             # share of each Z's trip spent fading in and out
SLEEP_NOSE_RIGHT = 440   # right edge of the sleeping head
SLEEP_GROUND = 290       # bottom of the sleeping cat

# run cat, in run pixel space
RUN_MATCH = 110 / 100    # sleep head width / run head width, so both cats match
RUN_TALL = 1.2           # the run art is long and low, so stretch it up a bit
RUN_NOSE_RIGHT = 505
RUN_GROUND = 340         # lowest foot across the run loop
RUN_FIRST = 7            # most crouched pose, the easiest one to bolt into
RUN_BOB = 10             # px the body lifts when the legs stretch out
RUN_ROCK = 2.5           # degrees the body tips nose up while lifting

# leap down to the bottom, in run pixel space
LEAP_POSE = 8            # run frame with the legs stretched out the most
LEAP_ANGLES = [2, 5, 7, 6, 3, 0, -2, -4, -5]  # eases nose up on take off, nose down falling
LAND_SQUASH = (1.04, 0.86)  # wider and shorter for one frame on landing

BODY = (255, 255, 255, 255)
INK = (0, 0, 0, 255)
FRINGE_DARK = 30         # fringe this dark becomes fully see-through
FRINGE_LIGHT = 225       # fringe this light stays fully solid


def fix_fringe(im):
    """Turn the dark rim left by background removal into soft white edges.

    Eyes and nose sit well inside the body, so they are kept as they are.
    """
    a = np.array(im).astype(np.float32)
    alpha, lum = a[..., 3], a[..., :3].mean(-1)
    solid = Image.fromarray(((alpha > 128) * 255).astype(np.uint8))
    inside = np.array(solid.filter(ImageFilter.MinFilter(9))) > 0
    ink = Image.fromarray((((lum < 90) & inside) * 255).astype(np.uint8))
    keep = np.array(ink.filter(ImageFilter.MaxFilter(5))) > 0

    light = np.clip((lum - FRINGE_DARK) / (FRINGE_LIGHT - FRINGE_DARK), 0, 1)
    out = a.copy()
    out[..., :3][~keep] = 255
    out[..., 3][~keep] = alpha[~keep] * light[~keep]
    return Image.fromarray(out.astype(np.uint8), "RGBA")


def no_zzz(im):
    im = im.copy()
    ImageDraw.Draw(im).rectangle(ZZZ_BOX, fill=(0, 0, 0, 0))
    return im


def open_eyes(im, height):
    im = im.copy()
    d = ImageDraw.Draw(im)
    for x0, y0, x1, y1 in CLOSED_EYE_BOXES:
        d.rectangle((x0 - EYE_GROW, y0 - EYE_GROW, x1 + EYE_GROW, y1 + EYE_GROW), fill=BODY)
    for cx, cy in EYES:
        d.ellipse((cx - EYE_W // 2, cy - height // 2, cx + EYE_W // 2, cy + height // 2), fill=INK)
    return im


def run_to_sleep_space(im):
    """Scale a run frame so it lines up with the sleeping cat's head and feet."""
    w = round(im.width * RUN_MATCH)
    h = round(im.height * RUN_MATCH * RUN_TALL)
    im = im.resize((w, h), Image.LANCZOS)
    x = SLEEP_NOSE_RIGHT - round(RUN_NOSE_RIGHT * RUN_MATCH)
    y = SLEEP_GROUND - round(RUN_GROUND * RUN_MATCH * RUN_TALL)
    return im, (x, y)


def clean(im):
    """Drop faint haze and hidden color in see-through pixels."""
    a = np.array(im)
    a[a[..., 3] < 8] = 0
    return Image.fromarray(a, "RGBA")


def bbox(im, at):
    b = im.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox()
    return (b[0] + at[0], b[1] + at[1], b[2] + at[0], b[3] + at[1])


def bold_closed_eyes(im):
    """Thicken the closed eye lines so they still show at 3 rows tall."""
    im = im.copy()
    for box in CLOSED_EYE_BOXES:
        grown = (box[0] - EYE_GROW, box[1] - EYE_GROW, box[2] + EYE_GROW, box[3] + EYE_GROW)
        part = im.crop(grown)
        rgb = part.convert("RGB").filter(ImageFilter.MinFilter(EYE_THICKEN))
        rgb.putalpha(part.getchannel("A"))
        im.paste(rgb, grown[:2])
    return im


def draw_z(im, cx, cy, size, alpha):
    """Draw one bold rounded Z centred on (cx, cy)."""
    layer = Image.new("RGBA", im.size, (0, 0, 0, 0))
    h = size / 2
    points = [(cx - h, cy - h), (cx + h, cy - h), (cx - h, cy + h), (cx + h, cy + h)]
    ImageDraw.Draw(layer).line(points, fill=(255, 255, 255, round(255 * alpha)),
                               width=Z_STROKE, joint="curve")
    for x, y in (points[0], points[-1]):
        r = Z_STROKE / 2
        ImageDraw.Draw(layer).ellipse((x - r, y - r, x + r, y + r),
                                      fill=(255, 255, 255, round(255 * alpha)))
    im.alpha_composite(layer)


def bold_zzz(im, i, count):
    """Three Z's rise from the head, grow and fade, looping every `count` frames."""
    im = im.copy()
    for k in range(Z_COUNT):
        t = (i / count + k / Z_COUNT) % 1.0
        x = Z_START[0] + (Z_END[0] - Z_START[0]) * t
        y = Z_START[1] + (Z_END[1] - Z_START[1]) * t
        size = Z_SMALL + (Z_BIG - Z_SMALL) * t
        alpha = min(1.0, t / Z_FADE, (1.0 - t) / Z_FADE)
        draw_z(im, x, y, size, alpha)
    return im


# each frame is (image, top-left position in sleep pixel space)
sleep_full = [bold_closed_eyes(no_zzz(f)) for f in
              (Image.open(p).convert("RGBA") for p in sleep_src)]
sleep_kept = sleep_full[::SLEEP_STEP]
sleep = [(bold_zzz(f, i, len(sleep_kept)), (0, 0)) for i, f in enumerate(sleep_kept)]

run_raw = [fix_fringe(Image.open(p).convert("RGBA")) for p in run_src]
run_raw = run_raw[RUN_FIRST:] + run_raw[:RUN_FIRST]


def spread(im):
    b = im.getchannel("A").point(lambda v: 255 if v > 128 else 0).getbbox()
    return b[2] - b[0]


def mass_center(im):
    """Middle of the cat, weighted by how solid each pixel is."""
    a = np.array(im.getchannel("A")).astype(np.float64)
    a[a < 8] = 0
    ys, xs = np.indices(a.shape)
    return (xs * a).sum() / a.sum(), (ys * a).sum() / a.sum()


def tilt(im, at, deg):
    """Rotate a frame around the cat's middle, keeping that middle in place."""
    cx, cy = mass_center(im)
    r = im.rotate(deg, resample=Image.BICUBIC, expand=True)
    r = r.crop(r.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox())
    rx, ry = mass_center(r)
    return r, (round(at[0] + cx - rx), round(at[1] + cy - ry))


# the body lifts and tips up as the legs stretch, and dips as they gather
widths = [spread(f) for f in run_raw]
lo, hi = min(widths), max(widths)
lift = [(w - lo) / (hi - lo) * 2 - 1 for w in widths]
n = len(lift)
lift = [(lift[k - 1] + 2 * lift[k] + lift[(k + 1) % n]) / 4 for k in range(n)]
run = []
for k, f in enumerate(run_raw):
    im, (x, y) = run_to_sleep_space(f)
    rising = (lift[(k + 1) % n] - lift[k - 1]) / 2
    im, (x, y) = tilt(im, (x, y), RUN_ROCK * rising * 2)
    run.append((im, (x, y - round(RUN_BOB * lift[k]))))

rest = sleep_full[0]
# wake is sudden: eyes snap wide open, hold a beat, hop, land
lying = open_eyes(rest, 28)
startled = open_eyes(rest, 32)
wake = [(rest, (0, 0)), (startled, (0, 0)), (startled, (0, 0)),
        (startled, (0, -12)), (lying, (0, 0))]


# leap: the stretched pose tips up then down, all around one fixed middle
pose, pose_at = run[LEAP_POSE]
leap = [tilt(pose, pose_at, deg) for deg in LEAP_ANGLES]

# keep the air frames inside the run frames' top and bottom, so the canvas
# doesn't grow. The shift is a few px while falling fast, so it hides.
sky = min(bbox(f, at)[1] for f, at in run)
ground = max(bbox(f, at)[3] for f, at in sleep + wake + run)
kept = []
for im, (x, y) in leap:
    b = bbox(im, (x, y))
    if b[3] - b[1] > ground - sky:
        y += round((sky + ground - b[1] - b[3]) / 2)
    else:
        y += max(sky - b[1], 0) + min(ground - b[3], 0)
    kept.append((im, (x, y)))
leap = kept

# landing: the run frame it leads into, squashed, same feet and same face
def eyes_x(im):
    a = np.array(im).astype(int)
    xs = np.where((a[..., 3] > 200) & (a[..., :3].mean(-1) < 60))[1]
    return xs.mean()


crouch, crouch_at = run[0]
sw, sh = LAND_SQUASH
land = crouch.resize((round(crouch.width * sw), round(crouch.height * sh)), Image.LANCZOS)
land_x = round(crouch_at[0] + eyes_x(crouch) - eyes_x(land))
leap.append((land, (land_x, crouch_at[1] + crouch.height - land.height)))

boxes = [bbox(f, at) for f, at in sleep + wake + run + leap]
PAD = 4
x0 = min(b[0] for b in boxes) - PAD
y0 = min(b[1] for b in boxes) - PAD
x1 = max(b[2] for b in boxes) + PAD
y1 = max(b[3] for b in boxes) + PAD


def on_canvas(im, at):
    c = Image.new("RGBA", (x1 - x0, y1 - y0), (0, 0, 0, 0))
    c.alpha_composite(im, (at[0] - x0, at[1] - y0))
    return c


wake_frames = [on_canvas(f, at) for f, at in wake]


def save(name, frames, ms):
    folder = out_root / name
    folder.mkdir(parents=True, exist_ok=True)
    for old in folder.glob("*.png"):
        old.unlink()
    done = []
    for i, f in enumerate(frames):
        f = f.resize((round(f.width * OUT_SCALE), round(f.height * OUT_SCALE)), Image.LANCZOS)
        f = clean(f)
        f.save(folder / f"{i:02}.png", optimize=True)
        done.append(f)
    print(f"{name}: {len(done)} frames, {done[0].size}, {ms} ms each")
    return done


sleep_out = save("sleep", [on_canvas(f, at) for f, at in sleep], 133)
wake_out = save("wake", wake_frames, 70)

# still preview: first sleep frame and every wake frame
row = sleep_out[:1] + wake_out
fw, fh = row[0].size
sheet = Image.new("RGBA", ((fw + 6) * len(row), fh + 12), (30, 31, 38, 255))
for i, f in enumerate(row):
    sheet.alpha_composite(f, (3 + i * (fw + 6), 6))
sheet.save(scratch / "preview.png")

# moving preview: two sleep loops, then the wake
W = fw
gif, durs = [], []


def add(f, x, ms):
    c = Image.new("RGBA", (W, fh), (30, 31, 38, 255))
    c.alpha_composite(f, (x, 0))
    gif.append(c.convert("RGB"))
    durs.append(ms)


for _ in range(2):
    for f in sleep_out:
        add(f, 0, 133)
for f in wake_out:
    add(f, 0, 70)
gif[0].save(scratch / "stray-sequence-preview.gif", save_all=True,
            append_images=gif[1:], duration=durs, loop=0)
