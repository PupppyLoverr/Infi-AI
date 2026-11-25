# Original "Silk" wallpaper for Solyra: broad folded sheets with lit creases.
import numpy as np, sys
from PIL import Image, ImageFilter
out, W, H, mode = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
y, x = np.mgrid[0:H, 0:W].astype(np.float32)
u, v = x / W, y / H
dark = mode == "dark"
def col(c): return np.array(c, np.float32)
if dark:
    sky_top, sky_bot = col([46, 24, 120]), col([20, 26, 92])
    sheets = [  # (edge fn params, body top colour, body bottom colour, rim strength)
        ((0.70, 0.22, 0.85, 0.4, -0.55), col([70, 60, 200]), col([18, 22, 90]), 1.0),
        ((0.88, 0.16, 1.10, 2.0, -0.40), col([40, 80, 220]), col([10, 14, 60]), 0.8),
        ((1.02, 0.10, 0.70, 4.2, -0.30), col([28, 40, 150]), col([8, 10, 40]), 0.6),
    ]
else:
    sky_top, sky_bot = col([226, 214, 252]), col([196, 206, 250])
    sheets = [
        ((0.70, 0.22, 0.85, 0.4, -0.55), col([190, 176, 250]), col([120, 130, 230]), 1.0),
        ((0.88, 0.16, 1.10, 2.0, -0.40), col([168, 196, 252]), col([96, 120, 220]), 0.8),
        ((1.02, 0.10, 0.70, 4.2, -0.30), col([200, 186, 246]), col([130, 140, 226]), 0.6),
    ]
t = np.clip(v, 0, 1)[..., None]
img = sky_top + (sky_bot - sky_top) * t
glow = np.exp(-(((u - 0.25) / 0.35) ** 2 + ((v - 0.15) / 0.3) ** 2))[..., None]
img += glow * (col([60, 40, 110]) if dark else col([20, 16, 4]))
for (cy, amp, fr, ph, tilt), ctop, cbot, rim in sheets:
    edge = cy + tilt * (u - 0.5) + amp * np.sin(2 * np.pi * fr * u + ph) * (0.6 + 0.4 * u)
    below = v - edge                                   # > 0 under the sheet's edge
    a = np.clip(below * H / 3.0, 0, 1)                 # anti-aliased edge
    depth = np.clip(below / 0.45, 0, 1)[..., None]
    body = ctop + (cbot - ctop) * depth ** 0.7
    # fold shading along x: darker where the sheet turns away
    turn = 0.82 + 0.18 * np.cos(2 * np.pi * fr * u + ph + 0.8)[..., None]
    body = body * turn
    # bright crease just under the edge, and a soft halo above it
    crease = np.exp(-((below * H) / 3.5) ** 2) * rim
    halo = np.exp(-((below * H + 6) / 40.0) ** 2) * 0.35 * rim * (below < 0)
    img = img * (1 - a[..., None]) + body * a[..., None]
    img += (crease + halo)[..., None] * (col([225, 230, 255]) if dark else col([120, 120, 130]))
rng = np.random.default_rng(11)
img += rng.normal(0, 1.0, img.shape)
im = Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))
im = im.filter(ImageFilter.GaussianBlur(0.6))
im.save(out, optimize=True)
