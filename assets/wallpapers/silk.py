# Original "Silk" wallpaper for Solyra: broad folded sheets with lit creases.
import numpy as np, sys
from PIL import Image, ImageFilter
out, W, H, mode = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
y, x = np.mgrid[0:H, 0:W].astype(np.float32)
u, v = x / W, y / H
dark = mode == "dark"
def col(c): return np.array(c, np.float32)
if dark:
    # Near-black navy with thin lit creases (the pearOS / macOS Tahoe Dark tone).
    sky_top, sky_bot = col([24, 20, 40]), col([14, 14, 28])
    sheets = [  # (edge fn params, body top colour, body bottom colour, rim strength)
        ((0.70, 0.22, 0.85, 0.4, -0.55), col([46, 38, 92]), col([14, 13, 30]), 0.85),
        ((0.88, 0.16, 1.10, 2.0, -0.40), col([34, 40, 100]), col([10, 11, 26]), 0.65),
        ((1.02, 0.10, 0.70, 4.2, -0.30), col([26, 26, 70]), col([8, 8, 20]), 0.5),
    ]
else:
    # Cream, taupe and lavender ribbons with bright white edges (the macOS
    # Tahoe-light genre): strong, thin highlights that show glass bending.
    sky_top, sky_bot = col([236, 228, 216]), col([206, 198, 204])
    sheets = [
        ((0.62, 0.24, 0.80, 0.4, -0.60), col([214, 198, 176]), col([150, 132, 120]), 1.0),
        ((0.82, 0.18, 1.05, 2.0, -0.45), col([196, 194, 218]), col([112, 110, 146]), 0.9),
        ((1.00, 0.12, 0.70, 4.2, -0.30), col([226, 214, 200]), col([168, 152, 140]), 0.8),
    ]
t = np.clip(v, 0, 1)[..., None]
img = sky_top + (sky_bot - sky_top) * t
glow = np.exp(-(((u - 0.25) / 0.35) ** 2 + ((v - 0.15) / 0.3) ** 2))[..., None]
img += glow * (col([34, 24, 64]) if dark else col([20, 16, 4]))
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
    img += (crease + halo)[..., None] * (col([200, 196, 250]) if dark else col([255, 255, 255]))
rng = np.random.default_rng(11)
img += rng.normal(0, 1.0, img.shape)
im = Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))
im = im.filter(ImageFilter.GaussianBlur(0.6))
im.save(out, optimize=True)
