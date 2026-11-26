# Original "Silk" wallpaper for Solyra: pearOS / macOS Sequoia-style
# sculptural 3D-lit champagne-gold, ivory-pearl, and deep slate-charcoal
# diagonal satin ribbons — smooth volumetric shading, no neon or harsh bands.
import numpy as np, sys
from PIL import Image, ImageFilter

out, W, H, mode = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
y, x = np.mgrid[0:H, 0:W].astype(np.float32)
u, v = x / W, y / H
dark = mode == "dark"


def col(c):
    return np.array(c, np.float32)


def smoothstep(e0, e1, t):
    k = np.clip((t - e0) / (e1 - e0), 0.0, 1.0)
    return k * k * (3.0 - 2.0 * k)


if dark:
    # Base: warm sunlit champagne-ivory upper centre fading into soft pearl-taupe
    base_tl = col([186, 174, 158])
    base_tr = col([218, 210, 198])
    base_bl = col([132, 122, 112])
    base_br = col([168, 162, 156])
else:
    base_tl = col([236, 230, 222])
    base_tr = col([248, 244, 238])
    base_bl = col([216, 210, 204])
    base_br = col([230, 226, 222])

top_row = base_tl * (1.0 - u[..., None]) + base_tr * u[..., None]
bot_row = base_bl * (1.0 - u[..., None]) + base_br * u[..., None]
img = top_row * (1.0 - v[..., None]) + bot_row * v[..., None]

# Soft sunlit bloom in upper-centre-right
sun = np.exp(-(((u - 0.56) / 0.36) ** 2 + ((v - 0.22) / 0.34) ** 2))[..., None]
img += sun * (col([26, 22, 16]) if dark else col([10, 8, 6]))

# Sculptural diagonal ribbon folds (modeled as curved 3D surfaces sweeping
# from upper-left / bottom-left to right, matching pearOS's architectural silk).
# Fold 1: Sweeping warm bronze-taupe arch on the left side
d1 = (u - 0.22) + 0.32 * v - 0.14 * np.sin(v * np.pi * 1.15)
m1 = smoothstep(-0.006, 0.006, -d1)
shade1 = np.clip(0.5 + 0.5 * np.tanh(-d1 * 5.2), 0.0, 1.0)[..., None]
c1_lit = col([212, 196, 174]) if dark else col([234, 224, 210])
c1_shd = col([96, 86, 76]) if dark else col([188, 180, 170])
fold1 = c1_shd + (c1_lit - c1_shd) * (shade1 ** 0.75)
# Soft ambient shadow cast right of fold 1
cast1 = np.exp(-np.clip(d1 / 0.055, 0.0, 6.0) ** 2) * (d1 > 0) * (0.22 if dark else 0.10)
img = img * (1.0 - cast1[..., None])
img = img * (1.0 - m1[..., None]) + fold1 * m1[..., None]

# Fold 2: Deep velvety slate-indigo / charcoal arch sweeping across centre-bottom
# (gives the dock and center desktop pearOS's signature dark satin contrast)
cx2 = 0.50 + 0.10 * v - 0.16 * np.sin((v - 0.15) * np.pi * 1.1)
d2 = (u - cx2) + 0.38 * (v - 0.62)
# Smooth elliptical arch boundary
arch2 = ((u - 0.44) / 0.34) ** 2 + ((v - 0.88) / 0.66) ** 2
m2 = smoothstep(1.012, 0.988, arch2)
# 3D cylindrical satin sheen across the dark slate arch
spec2 = np.exp(-((u - 0.48 - 0.08 * v) / 0.22) ** 2)[..., None]
c2_deep = col([42, 44, 56]) if dark else col([156, 158, 172])
c2_mid = col([78, 80, 96]) if dark else col([194, 196, 208])
c2_hi = col([128, 128, 144]) if dark else col([226, 228, 236])
fold2 = c2_deep + (c2_mid - c2_deep) * (1.0 - v[..., None] * 0.45) + (c2_hi - c2_mid) * spec2 * 0.55
# Soft rim shadow & crisp delicate 1.5px glass-silk rim along arch2
edge2 = np.exp(-((arch2 - 1.0) / 0.012) ** 2) * (0.42 if dark else 0.25)
cast2 = np.exp(-np.clip((arch2 - 1.0) / 0.14, 0.0, 6.0) ** 2) * (arch2 > 1.0) * (0.20 if dark else 0.08)
img = img * (1.0 - cast2[..., None])
img = img * (1.0 - m2[..., None]) + fold2 * m2[..., None]
img += edge2[..., None] * col([238, 234, 228])

# Fold 3: Graceful silver-champagne ribbon sweeping in from top-right to bottom-right
arch3 = ((u - 0.96) / 0.34) ** 2 + ((v - 0.52) / 0.72) ** 2
m3 = smoothstep(1.014, 0.986, arch3)
spec3 = np.exp(-((u - 0.80) / 0.16) ** 2)[..., None]
c3_base = col([156, 152, 148]) if dark else col([216, 212, 208])
c3_hi = col([224, 218, 210]) if dark else col([246, 242, 238])
fold3 = c3_base + (c3_hi - c3_base) * (0.35 + 0.65 * spec3)
edge3 = np.exp(-((arch3 - 1.0) / 0.010) ** 2) * (0.38 if dark else 0.22)
cast3 = np.exp(-np.clip((arch3 - 1.0) / 0.12, 0.0, 6.0) ** 2) * (arch3 > 1.0) * (0.16 if dark else 0.07)
img = img * (1.0 - cast3[..., None])
img = img * (1.0 - m3[..., None]) + fold3 * m3[..., None]
img += edge3[..., None] * col([245, 240, 234])

# Fold 4: Lower-left warm bronze-gold ribbon tucking under the slate arch
arch4 = ((u + 0.04) / 0.38) ** 2 + ((v - 0.92) / 0.58) ** 2
m4 = smoothstep(1.015, 0.985, arch4)
c4_base = col([86, 78, 70]) if dark else col([186, 178, 168])
c4_hi = col([178, 162, 142]) if dark else col([224, 216, 206])
spec4 = np.exp(-((arch4 - 0.85) / 0.25) ** 2)[..., None]
fold4 = c4_base + (c4_hi - c4_base) * spec4 * 0.75
edge4 = np.exp(-((arch4 - 1.0) / 0.010) ** 2) * (0.35 if dark else 0.20)
img = img * (1.0 - m4[..., None]) + fold4 * m4[..., None]
img += edge4[..., None] * col([240, 230, 216])

# Subtle photographic micro-dither to eliminate banding
rng = np.random.default_rng(23)
img += rng.normal(0, 0.75, img.shape)
im = Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))
im = im.filter(ImageFilter.GaussianBlur(0.8))
im.save(out, optimize=True)
