"""Bake the Solar icons Solyra uses into 48 px alpha masks.

Solar (Linear) by 480 Design, CC BY 4.0, the set zeron uses for its UI.
Source: the @iconify-json/solar package (icons.json). Usage:
    python3 bake_solar.py path/to/icons.json
The panel tints each mask at draw time (see `Canvas::glyph`).
"""
import io, json, sys
import cairosvg
from PIL import Image

GLYPHS = {
    "search": "magnifier-linear",
    "wifi": "wi-fi-router-minimalistic-linear",
    "wifi-full": "wi-fi-high-linear",
    "control": "tuning-2-linear",
    "moon": "moon-linear",
    "sun": "sun-2-linear",
    "bell": "bell-linear",
    "lock": "lock-keyhole-minimalistic-linear",
    "power": "power-linear",
    "cloud": "cloud-linear",
    "note": "music-note-2-linear",
    "laptop": "laptop-minimalistic-linear",
    "volume": "volume-loud-linear",
    "volume-low": "volume-small-linear",
    "volume-mute": "volume-cross-linear",
    "bluetooth": "bluetooth-linear",
    "monitor": "monitor-linear",
    "agents": "cpu-bolt-linear",
    "settings": "settings-minimalistic-linear",
    "home": "home-2-linear",
    "folder": "folder-2-linear",
    "download": "download-minimalistic-linear",
    "document": "document-text-linear",
    "gallery": "gallery-linear",
    "star": "star-linear",
    "trash": "trash-bin-minimalistic-linear",
    "clock": "clock-circle-linear",
    "widget": "widget-4-linear",
    "tag": "tag-linear",
    "share": "share-linear",
    "server": "server-minimalistic-linear",
}

icons = json.load(open(sys.argv[1]))
data, W, H = icons["icons"], icons.get("width", 24), icons.get("height", 24)
for name, key in GLYPHS.items():
    if key not in data:
        cands = [k for k in data if k.endswith("-linear") and key.split("-")[0] in k]
        key = cands[0] if cands else None
    if key is None:
        print("missing", name); continue
    body = data[key]["body"].replace("currentColor", "#000")
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 {W} {H}">{body}</svg>'
    png = cairosvg.svg2png(bytestring=svg.encode(), output_width=48, output_height=48)
    Image.open(io.BytesIO(png)).convert("RGBA").save(f"{name}.png", optimize=True)
    print(name, key)
