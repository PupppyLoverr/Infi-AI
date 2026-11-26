# Original "Silk" wallpaper for Solyra: pearOS / macOS Sequoia-grade
# champagne-gold, warm pearl-silver, and deep charcoal-slate satin ribbons.
import numpy as np, sys
from PIL import Image, ImageFilter

out, W, H, mode = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
y, x = np.mgrid[0:H, 0:W].astype(np.float32)
u, v = x / W, y / H
dark = mode == "dark"


def col(c):
    return np.array(c, np.float32)


if dark:
    # Warm champagne / pearl-beige top-left flowing into rich satin slate-charcoal
    bg_tl, bg_tr = col([192, 178, 160]), col([216, 206, 194])
    bg_bl, bg_br = col([78, 74, 72]), col([132, 126, 122])
    sheets = [
        # Broad warm bronze-champagne sweep across left & upper-middle
        ((0.46, 0.24, 0.72, 0.55, 0.42), col([148, 134, 118]), col([58, 56, 62]), 0.85, col([246, 238, 226])),
        # Deep satin charcoal-slate ribbon sweeping across centre & bottom
        ((0.68, 0.20, 0.85, 2.35, -0.36), col([68, 70, 82]), col([24, 26, 34]), 0.95, col([235, 230, 224])),
        # Lower cool graphite-silver silk fold under the dock
        ((0.88, 0.14, 0.65, 4.10, -0.22), col([92, 94, 108]), col([34, 36, 46]), 0.75, col([242, 238, 232])),
    ]
else:
    bg_tl, bg_tr = col([236, 228, 218]), col([246, 242, 236])
    bg_bl, bg_br = col([212, 204, 194]), col([226, 222, 218])
    sheets = [
        ((0.46, 0.24, 0.72, 0.55, 0.42), col([224, 214, 202]), col([186, 180, 174]), 0.65, col([255, 252, 248])),
        ((0.68, 0.20, 0.85, 2.35, -0.36), col([198, 196, 204]), col([158, 158, 168]), 0.70, col([252, 250, 248])),
        ((0.88, 0.14, 0.65, 4.10, -0.22), col([214, 212, 218]), col([174, 174, 184]), 0.55, col([255, 255, 255])),
    ]

# Bilinear warm-pearl base gradient
top_row = bg_tl * (1.0 - u[..., None]) + bg_tr * u[..., None]
bot_row = bg_bl * (1.0 - u[..., None]) + bg_br * u[..., None]
img = top_row * (1.0 - v[..., None]) + bot_row * v[..., None]

# Soft sunlit pearl bloom near upper-centre-right (gives glass behind menu bar & notch luminous depth)
bloom = np.exp(-(((u - 0.58) / 0.38) ** 2 + ((v - 0.14) / 0.28) ** 2))[..., None]
img += bloom * (col([36, 32, 26]) if dark else col([14, 12, 10]))

for (cy, amp, fr, ph, tilt), ctop, cbot, rim, rim_col in sheets:
    edge = cy + tilt * (u - 0.5) + amp * np.sin(2.0 * np.pi * fr * u + ph) * (0.65 + 0.35 * u)
    below = v - edge
    a = np.clip(below * H / 3.5, 0.0, 1.0)
    depth = np.clip(below / 0.52, 0.0, 1.0)[..., None]
    body = ctop + (cbot - ctop) * (depth ** 0.72)
    # Satin directional sheen along the curve
    turn = 0.86 + 0.14 * np.cos(2.0 * np.pi * fr * u + ph + 0.7)[..., None]
    body = body * turn
    # Crisp luminous silk edge + soft satin falloff
    crease = np.exp(-((below * H) / 4.2) ** 2) * rim
    halo = np.exp(-((below * H + 8.0) / 52.0) ** 2) * 0.28 * rim * (below < 0)
    img = img * (1.0 - a[..., None]) + body * a[..., None]
    img += (crease + halo)[..., None] * rim_col * 0.55

rng = np.random.default_rng(11)
img += rng.normal(0, 0.85, img.shape)
im = Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))
im = im.filter(ImageFilter.GaussianBlur(0.6))
im.save(out, optimize=True)
