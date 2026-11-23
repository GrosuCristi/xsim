"""Regenerates xsim's logo and icon assets from the owner's logo (D20).

The source is the owner's logo: a white "xsim" over a blue wave, on black.
Run with Python 3 and Pillow (no other packages):

    python -I make_logo.py [--source FILE] [--out DIR] [--preview DIR]

By default it reads ``logo-source.webp`` next to this script and writes
next to it:

* ``logo-dark-<H>.png``: the wide logo on a transparent background, white
  letters, for the dark theme; ``<H>`` is the height in pixels.
* ``logo-light-<H>.png``: the same with dark letters, for the light theme.
* ``icon-<N>.png``: the square app icon, N x N pixels.
* ``xsim.ico``: every icon size in one Windows icon (BMP frames up to
  48 px, PNG frames above, as Windows' own tools write them).

``--preview DIR`` also writes the logos and icons over the light and dark
title-bar colours, for a look before committing.
"""

import argparse
import io
import math
import os
import struct
from collections import deque

from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))

# The logo's colours, measured from the source.
WAVE = (54, 118, 253)
SOURCE_WHITE = 253.0
# Letter colours of the two variants: the source white, and neutral-900
# (a step lighter than the kit's light-theme text, neutral-950) for light
# title bars.
LETTERS_DARK_THEME = (253, 253, 253)
LETTERS_LIGHT_THEME = (23, 23, 23)

# Heights of the wide logo. The title bar shows it about 24 px high; the
# app picks the smallest one that covers its height in device pixels, so
# the GPU never shrinks one by more than 1.5x (it has no mipmaps). At
# 100 % on a 100 % display the 24 px one is drawn texel for texel.
LOGO_HEIGHTS = (24, 32, 48, 64, 96, 128)

ICON_SIZES = (16, 24, 32, 48, 64, 128, 256)
# Icon frames up to this size are stored as BMP in the .ico.
ICO_BMP_UP_TO = 48
ICON_MASTER = 512
TILE = (12, 13, 17)

# Title-bar backgrounds of the kit's default light and dark themes.
PREVIEW_BACKGROUNDS = {"light": (248, 248, 248), "dark": (23, 23, 23)}


def clamp01(value):
    return 0.0 if value < 0.0 else 1.0 if value > 1.0 else value


def remap(value, low, high):
    """Maps [low, high] to [0, 1]: drops compression noise near 0 and 1."""
    return clamp01((value - low) / (high - low))


class Layers:
    """The source split into letter coverage and wave coverage (0..1).

    Over black, a pixel is ``a * white + (1 - a) * v * WAVE``: ``a`` is the
    letters' coverage and ``v`` the wave's (darkened where the letters
    shadow it). Red and blue tell the two apart, since white has equal
    channels and the wave's differ.
    """

    def __init__(self, source):
        rgb = source.convert("RGB")
        lit = rgb.convert("L").point(lambda v: 255 if v > 8 else 0)
        left, top, right, bottom = lit.getbbox()
        margin = 4
        self.box = (
            max(0, left - margin),
            max(0, top - margin),
            min(rgb.width, right + margin),
            min(rgb.height, bottom + margin),
        )
        crop = rgb.crop(self.box)
        self.size = crop.size
        width, height = self.size
        pixels = crop.load()
        self.letters = [[0.0] * width for _ in range(height)]
        self.wave = [[0.0] * width for _ in range(height)]
        span = WAVE[2] - WAVE[0]
        for y in range(height):
            letters, wave = self.letters[y], self.wave[y]
            for x in range(width):
                r, _, b = pixels[x, y]
                u = max(0.0, (b - r) / span)
                a = clamp01((r - u * WAVE[0]) / SOURCE_WHITE)
                v = u / (1.0 - a) if a < 0.98 else 0.0
                # Chroma subsampling leaves a faint grey line along the
                # wave's edges that reads as letter coverage; drop it.
                letters[x] = remap(a, 0.16, 0.97)
                wave[x] = remap(v, 0.03, 0.97)

    def mask(self, rows):
        """A coverage grid as an 'L' image."""
        width, height = self.size
        image = Image.new("L", (width, height))
        image.putdata([round(255 * v) for row in rows for v in row])
        return image


def wide_logo(layers, letter_color):
    """The logo with `letter_color` letters on a transparent background.

    The wave keeps the source's shading: next to the letters it fades out,
    which over a dark title bar looks like the source's shadow and over a
    light one separates the dark letters from the wave.
    """
    width, height = layers.size
    out = Image.new("RGBA", (width, height))
    data = []
    for y in range(height):
        for x in range(width):
            a, v = layers.letters[y][x], layers.wave[y][x]
            wave = (1.0 - a) * v
            alpha = a + wave
            if alpha <= 0.0:
                data.append((0, 0, 0, 0))
                continue
            color = tuple(
                round((a * letter_color[i] + wave * WAVE[i]) / alpha) for i in range(3)
            )
            data.append(color + (round(255 * alpha),))
    out.putdata(data)
    return out.crop(out.getchannel("A").getbbox())


def resize(image, size):
    """Resamples with premultiplied alpha, so edges get no dark fringe."""
    return image.convert("RGBa").resize(size, Image.LANCZOS).convert("RGBA")


# Texels less opaque than this take their colour from the drawing next to
# them (see `bleed`).
BLEED_BELOW = 16
# How many texels out from the drawing get its colour. Bilinear sampling
# reaches one texel; the second is a margin.
BLEED_RINGS = 2


def bleed(image):
    """Gives the (nearly) transparent texels around the drawing the colour of
    the drawing next to them, keeping their alpha.

    GPUI draws an image with straight alpha and bilinear sampling, so where
    it shows a rendering a little smaller than the PNG, a sample mixes in
    the colour of the transparent texels beside the drawing too. Left black
    (as premultiplied resampling leaves them), they would put a dark rim on
    the white letters and the wave.
    """
    image = image.copy()
    width, height = image.size
    pixels = image.load()
    known = [[pixels[x, y][3] >= BLEED_BELOW for x in range(width)] for y in range(height)]
    for _ in range(BLEED_RINGS):
        ring = []
        for y in range(height):
            for x in range(width):
                if known[y][x]:
                    continue
                total, weight = [0.0, 0.0, 0.0], 0.0
                for ny in range(max(0, y - 1), min(height, y + 2)):
                    for nx in range(max(0, x - 1), min(width, x + 2)):
                        if known[ny][nx]:
                            r, g, b, a = pixels[nx, ny]
                            w = max(a, 1)
                            total = [total[0] + w * r, total[1] + w * g, total[2] + w * b]
                            weight += w
                if weight:
                    ring.append((x, y, tuple(round(c / weight) for c in total)))
        for x, y, color in ring:
            pixels[x, y] = color + (pixels[x, y][3],)
            known[y][x] = True
    return image


# --- Icon ---------------------------------------------------------------


def column_edges(wave, x, threshold=0.5):
    """Top and bottom edge (sub-pixel) of the wave in column `x`."""
    height = len(wave)
    rows = [wave[y][x] for y in range(height)]
    inside = [y for y in range(height) if rows[y] >= threshold]
    if not inside:
        return None
    top, bottom = inside[0], inside[-1]

    def crossing(outer, inner):
        span = rows[inner] - rows[outer]
        return inner - (inner - outer) * (rows[inner] - threshold) / span if span else inner

    top_edge = crossing(top - 1, top) if top > 0 else float(top)
    bottom_edge = crossing(bottom + 1, bottom) if bottom + 1 < height else float(bottom)
    return top_edge, bottom_edge


def smooth(values, window):
    half = window // 2
    out = []
    for i in range(len(values)):
        lo, hi = max(0, i - half), min(len(values), i + half + 1)
        out.append(sum(values[lo:hi]) / (hi - lo))
    return out


def wave_columns(layers):
    """(x, (top, bottom)) for every crop column that shows the wave."""
    columns = []
    for x in range(layers.size[0]):
        edges = column_edges(layers.wave, x)
        if edges is not None:
            columns.append((x, edges))
    return columns


def wave_middle(layers):
    """The wave's middle (where it crosses its centre line): halfway
    between the crest's and the trough's centre."""
    half = layers.size[0] / 2
    centres = [(x, (t + b) / 2) for x, (t, b) in wave_columns(layers)]
    crest = min((c for c in centres if c[0] < half), key=lambda c: c[1])
    trough = max((c for c in centres if c[0] > half), key=lambda c: c[1])
    return (crest[0] + trough[0]) / 2, (crest[1] + trough[1]) / 2


def crest_lobe(layers):
    """The centre line and half-width of the wave's first lobe (the crest),
    measured column by column where nothing covers it: (x, y, r) points in
    crop pixels, from the left tip towards the middle of the logo.

    The source shows this lobe whole up to where it dives behind the "s".
    """
    columns = []
    for x, edges in wave_columns(layers):
        if columns and x != columns[-1][0] + 1:
            break
        columns.append((x, edges))
    # Stop before the letters' shadow: where the stroke starts thinning
    # fast while still descending.
    xs = [x for x, _ in columns]
    centres = [(t + b) / 2 for _, (t, b) in columns]
    thick = [b - t for _, (t, b) in columns]
    crest = centres.index(min(centres))
    end = crest
    while end + 1 < len(columns) and thick[end + 1] > 0.8 * thick[crest]:
        end += 1
    end = max(crest, end - 6)
    xs, centres, thick = xs[: end + 1], smooth(centres[: end + 1], 9), smooth(thick[: end + 1], 9)
    points = []
    # Columns cut the steep stroke at the tip at a slant, so the measured
    # width does not reach zero there; the source's tip is pointed.
    taper = max(1, len(xs) // 10)
    for i, x in enumerate(xs):
        lo, hi = max(0, i - 3), min(len(xs) - 1, i + 3)
        slope = (centres[hi] - centres[lo]) / (xs[hi] - xs[lo]) if hi > lo else 0.0
        radius = thick[i] / 2 / math.sqrt(1 + slope * slope)
        if i < taper:
            t = i / taper
            radius *= 0.15 + 0.85 * t * t * (3 - 2 * t)
        points.append((float(x), centres[i], radius))
    return points


def hermite(p0, p1, t0, t1, steps):
    """Points of the cubic Hermite curve from p0 to p1 with tangents t0, t1."""
    out = []
    for i in range(1, steps):
        s = i / steps
        h00, h10 = 2 * s**3 - 3 * s**2 + 1, s**3 - 2 * s**2 + s
        h01, h11 = -2 * s**3 + 3 * s**2, s**3 - s**2
        out.append(
            tuple(h00 * p0[k] + h10 * t0[k] + h01 * p1[k] + h11 * t1[k] for k in range(2))
        )
    return out


def full_wave(lobe, middle):
    """One whole period: the crest lobe, the same lobe turned half a turn
    about the wave's `middle` for the trough (the source's trough is that
    shape), and a smooth bridge between them where the source hides the
    wave behind the letters."""
    last, before = lobe[-1], lobe[-8]

    def turned(point):
        x, y, r = point
        return 2 * middle[0] - x, 2 * middle[1] - y, r

    trough = [turned(p) for p in reversed(lobe)]
    tangent = (last[0] - before[0], last[1] - before[1])
    norm = math.hypot(*tangent)
    gap = math.hypot(trough[0][0] - last[0], trough[0][1] - last[1])
    tangent = (tangent[0] / norm * gap, tangent[1] / norm * gap)
    steps = max(2, int(gap))
    bridge = hermite(last[:2], trough[0][:2], tangent, tangent, steps)
    # The stroke thins towards the middle a little, as in the source.
    narrowest = 0.85 * last[2]
    bridge = [
        (x, y, last[2] + (narrowest - last[2]) * math.sin(math.pi * (i + 1) / steps))
        for i, (x, y) in enumerate(bridge)
    ]
    return lobe + bridge + trough


def draw_stroke(points, size, scale, offset, extra, supersample=4):
    """The wave as an 'L' mask: discs of radius r + `extra` along the
    centre line, drawn `supersample` times larger and reduced. `scale` is
    (x, y, radius)."""
    big = Image.new("L", (size * supersample, size * supersample))
    draw = ImageDraw.Draw(big)
    dense = []
    for (x0, y0, r0), (x1, y1, r1) in zip(points, points[1:]):
        length = math.hypot((x1 - x0) * scale[0], (y1 - y0) * scale[1])
        steps = max(1, int(length * supersample / 2))
        for i in range(steps):
            t = i / steps
            dense.append((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, r0 + (r1 - r0) * t))
    dense.append(points[-1])
    for x, y, r in dense:
        cx = (x * scale[0] + offset[0]) * supersample
        cy = (y * scale[1] + offset[1]) * supersample
        radius = (r * scale[2] + extra) * supersample
        if radius <= 0:
            continue
        draw.ellipse((cx - radius, cy - radius, cx + radius, cy + radius), fill=255)
    return big.resize((size, size), Image.LANCZOS)


def letter_x(layers):
    """The coverage of the logo's "x" alone, cropped to it."""
    width, height = layers.size
    letters = layers.letters
    # The first solid letter pixel from the left belongs to the "x".
    seed = min(
        ((x, y) for y in range(height) for x in range(width) if letters[y][x] > 0.9),
        key=lambda p: p[0],
    )
    seen = {seed}
    queue = deque([seed])
    while queue:
        x, y = queue.popleft()
        for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if 0 <= nx < width and 0 <= ny < height and (nx, ny) not in seen:
                if letters[ny][nx] > 0.5:
                    seen.add((nx, ny))
                    queue.append((nx, ny))
    solid = Image.new("L", (width, height))
    solid.putdata([255 if (x, y) in seen else 0 for y in range(height) for x in range(width)])
    # The component plus its anti-aliased rim.
    rim = solid.filter(ImageFilter.MaxFilter(5))
    glyph = ImageChops.multiply(rim, layers.mask(letters))
    return glyph.crop(glyph.getbbox())


def rounded_tile(size, radius, supersample=4):
    big = Image.new("L", (size * supersample, size * supersample))
    ImageDraw.Draw(big).rounded_rectangle(
        (0, 0, size * supersample - 1, size * supersample - 1),
        radius=radius * supersample,
        fill=255,
    )
    return big.resize((size, size), Image.LANCZOS)


def solid(size, color, mask):
    layer = Image.new("RGBA", (size, size), color + (255,))
    layer.putalpha(mask)
    return layer


def icon_master(wave_points, glyph, frame):
    """The icon at ICON_MASTER px, drawn for a `frame`-pixel icon: small
    frames get a thicker wave and a larger "x", so both stay legible."""
    size = ICON_MASTER
    # Extra half-width of the wave, and the height of the x, per frame size.
    if frame <= 16:
        extra, x_height = 0.05, 0.40
    elif frame <= 24:
        extra, x_height = 0.035, 0.38
    elif frame <= 32:
        extra, x_height = 0.022, 0.36
    elif frame <= 48:
        extra, x_height = 0.01, 0.35
    else:
        extra, x_height = 0.0, 0.34
    xs = [p[0] for p in wave_points]
    ys = [p[1] for p in wave_points]
    # The logo's wave is twice as wide as high; the icon stands it up to
    # fill the square, so the crest clears the "x" above and the trough
    # below it.
    scale_x = 0.80 * size / (max(xs) - min(xs))
    scale_y = 0.56 * size / (max(ys) - min(ys))
    scale = (scale_x, scale_y, math.sqrt(scale_x * scale_y))
    centre = ((min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2)
    offset = (size / 2 - centre[0] * scale_x, size / 2 - centre[1] * scale_y)
    wave = draw_stroke(wave_points, size, scale, offset, extra * size)

    glyph_height = round(x_height * size)
    glyph_width = round(glyph.width * glyph_height / glyph.height)
    glyph = glyph.resize((glyph_width, glyph_height), Image.LANCZOS)
    x_mask = Image.new("L", (size, size))
    x_mask.paste(glyph, ((size - glyph_width) // 2, (size - glyph_height) // 2))

    # The source's shadow: the letters darken the wave around them.
    shadow = x_mask.filter(ImageFilter.MaxFilter(9)).filter(
        ImageFilter.GaussianBlur(size * 0.02)
    )
    lit = ImageChops.multiply(wave, ImageChops.invert(shadow))

    tile_mask = rounded_tile(size, round(0.22 * size))
    icon = solid(size, TILE, tile_mask)
    icon = Image.alpha_composite(icon, solid(size, WAVE, ImageChops.multiply(lit, tile_mask)))
    icon = Image.alpha_composite(icon, solid(size, LETTERS_DARK_THEME, x_mask))
    return icon


def write_ico(path, frames):
    """A Windows icon with `frames` (size -> RGBA image)."""
    entries, blobs = [], []
    for size in sorted(frames):
        image = frames[size]
        if size <= ICO_BMP_UP_TO:
            blob = bmp_frame(image)
        else:
            buffer = io.BytesIO()
            image.save(buffer, "PNG", optimize=True)
            blob = buffer.getvalue()
        entries.append(size)
        blobs.append(blob)
    offset = 6 + 16 * len(entries)
    out = bytearray(struct.pack("<HHH", 0, 1, len(entries)))
    for size, blob in zip(entries, blobs):
        dim = 0 if size >= 256 else size
        out += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    for blob in blobs:
        out += blob
    with open(path, "wb") as file:
        file.write(out)


def bmp_frame(image):
    """A 32-bit BMP icon frame: header, bottom-up BGRA rows, AND mask."""
    width, height = image.size
    header = struct.pack("<IiiHHIIiiII", 40, width, height * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = image.load()
    rows = bytearray()
    for y in reversed(range(height)):
        for x in range(width):
            r, g, b, a = pixels[x, y]
            rows += bytes((b, g, r, a))
    stride = ((width + 31) // 32) * 4
    mask = bytearray()
    for y in reversed(range(height)):
        row = bytearray(stride)
        for x in range(width):
            if pixels[x, y][3] == 0:
                row[x // 8] |= 0x80 >> (x % 8)
        mask += row
    return header + bytes(rows) + bytes(mask)


def preview(path, image, background, zoom):
    canvas = Image.new("RGBA", image.size, background + (255,))
    canvas = Image.alpha_composite(canvas, image)
    if zoom != 1:
        canvas = canvas.resize((image.width * zoom, image.height * zoom), Image.NEAREST)
    canvas.convert("RGB").save(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--source", default=os.path.join(HERE, "logo-source.webp"))
    parser.add_argument("--out", default=HERE)
    parser.add_argument("--preview")
    args = parser.parse_args()

    layers = Layers(Image.open(args.source))
    written = []
    for theme, letters in (("dark", LETTERS_DARK_THEME), ("light", LETTERS_LIGHT_THEME)):
        logo = wide_logo(layers, letters)
        for height in LOGO_HEIGHTS:
            width = round(logo.width * height / logo.height)
            name = os.path.join(args.out, f"logo-{theme}-{height}.png")
            image = bleed(resize(logo, (width, height)))
            image.save(name, optimize=True)
            written.append(name)
            if args.preview:
                for where, background in PREVIEW_BACKGROUNDS.items():
                    preview(
                        os.path.join(args.preview, f"logo-{theme}-{height}-on-{where}.png"),
                        image,
                        background,
                        4 if height <= 48 else 1,
                    )

    wave_points = full_wave(crest_lobe(layers), wave_middle(layers))
    glyph = letter_x(layers)
    frames = {}
    for size in ICON_SIZES:
        master = icon_master(wave_points, glyph, size)
        frames[size] = resize(master, (size, size))
        name = os.path.join(args.out, f"icon-{size}.png")
        frames[size].save(name, optimize=True)
        written.append(name)
        if args.preview:
            for where, background in PREVIEW_BACKGROUNDS.items():
                preview(
                    os.path.join(args.preview, f"icon-{size}-on-{where}.png"),
                    frames[size],
                    background,
                    max(1, 256 // size),
                )
    ico = os.path.join(args.out, "xsim.ico")
    write_ico(ico, frames)
    written.append(ico)
    for name in written:
        print(name, os.path.getsize(name), "bytes")


if __name__ == "__main__":
    main()
