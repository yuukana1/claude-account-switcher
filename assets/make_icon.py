"""Generate the app icon: warm orange tile with the Claude-style starburst."""

import math
import os

from PIL import Image, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
S = 1024
SS = 4  # supersampling
W = S * SS

img = Image.new("RGBA", (W, W), (0, 0, 0, 0))

# --- rounded-square gradient tile -------------------------------------------
tile = Image.new("RGBA", (W, W), (0, 0, 0, 0))
grad = Image.new("RGBA", (W, W), (0, 0, 0, 0))
gd = ImageDraw.Draw(grad)
top = (232, 150, 118)
bottom = (188, 88, 60)
for y in range(W):
    t = y / (W - 1)
    # ease the blend slightly toward the middle for a softer light source
    te = t * t * (3 - 2 * t)
    c = tuple(int(top[i] + (bottom[i] - top[i]) * te) for i in range(3)) + (255,)
    gd.line([(0, y), (W, y)], fill=c)

mask = Image.new("L", (W, W), 0)
md = ImageDraw.Draw(mask)
md.rounded_rectangle([0, 0, W - 1, W - 1], radius=int(W * 0.225), fill=255)
tile.paste(grad, (0, 0), mask)

# soft top highlight
hl = Image.new("RGBA", (W, W), (0, 0, 0, 0))
hd = ImageDraw.Draw(hl)
hd.ellipse(
    [-W * 0.25, -W * 0.62, W * 1.25, W * 0.30],
    fill=(255, 255, 255, 46),
)
hl = hl.filter(ImageFilter.GaussianBlur(W * 0.03))
tile.alpha_composite(Image.composite(hl, Image.new("RGBA", (W, W), (0, 0, 0, 0)), mask))

# --- starburst ---------------------------------------------------------------
rays = Image.new("RGBA", (W, W), (0, 0, 0, 0))
rd = ImageDraw.Draw(rays)
cx = cy = W / 2.0
n = 12
inner = W * 0.058
outer = W * 0.408
width = int(W * 0.047)
for i in range(n):
    a = math.radians(-90 + i * 360.0 / n)
    x0 = cx + math.cos(a) * inner
    y0 = cy + math.sin(a) * inner
    x1 = cx + math.cos(a) * outer
    y1 = cy + math.sin(a) * outer
    rd.line([(x0, y0), (x1, y1)], fill=(255, 250, 246, 255), width=width)
    r = width / 2.0
    rd.ellipse([x1 - r, y1 - r, x1 + r, y1 + r], fill=(255, 250, 246, 255))
    rd.ellipse([x0 - r, y0 - r, x0 + r, y0 + r], fill=(255, 250, 246, 255))

tile.alpha_composite(rays)

img = tile.resize((S, S), Image.LANCZOS)
img.save(os.path.join(HERE, "icon.png"))

sizes = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
frames = [img.resize((s, s), Image.LANCZOS) for s in sizes]
frames[-1].save(
    os.path.join(HERE, "icon.ico"),
    format="ICO",
    sizes=[(s, s) for s in sizes],
    append_images=frames[:-1],
)
print("wrote icon.png and icon.ico")
