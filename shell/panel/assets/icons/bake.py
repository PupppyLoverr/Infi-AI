#!/usr/bin/env python3
"""Re-bake the bundled dock icons from an installed WhiteSur icon theme.

    python3 bake.py [/usr/share/icons/WhiteSur]

Needs headless Chrome (to rasterise the SVGs with alpha) and Pillow. Each
icon is rendered at 256 px, then resampled to 192 px with the squircle body
inset 12 px on every side (the panel relies on that 168/192 body ratio, see
`icons::BODY`).
"""
import json
import os
import subprocess
import sys

from PIL import Image

THEME = sys.argv[1] if len(sys.argv) > 1 else "/usr/share/icons/WhiteSur"
OUT = os.path.dirname(os.path.abspath(__file__))
CELL, COLS, FINAL = 256, 6, 192

ICONS = {
    "files": "places/scalable/folder.svg",
    "terminal": "apps/scalable/terminal.svg",
    "browser": "local:browser.svg",
    "firefox": "apps/scalable/firefox.svg",
    "chrome": "apps/scalable/chrome.svg",
    "settings": "apps/scalable/preferences-system.svg",
    "overview": "apps/scalable/preferences-system-windows.svg",
    "editor": "apps/scalable/text-editor.svg",
    "trash": "places/scalable/user-trash.svg",
    "wallpaper": "apps/scalable/preferences-desktop-wallpaper.svg",
    "calculator": "apps/scalable/calc.svg",
    "music": "apps/scalable/gnome-music.svg",
    "photos": "apps/scalable/shotwell.svg",
    "mail": "apps/scalable/internet-mail.svg",
    "appstore": "apps/scalable/softwarecenter.svg",
    "monitor": "apps/scalable/utilities-system-monitor.svg",
    "notes": "apps/scalable/basket.svg",
    "maps": "apps/scalable/gnome-maps.svg",
    "launchpad": "apps/scalable/view-app-grid.svg",
    "downloads": "places/scalable/folder-download.svg",
    "viewer": "apps/scalable/org.gnome.Loupe.svg",
}


# Icons that are a free-standing shape rather than a squircle body (the folder)
# are scaled so their bounding box fits the same 168/192 body as the rest.
FIT_TO_BODY = {"files", "downloads"}


def fit_to_body(tile):
    box = tile.getchannel("A").getbbox()
    if not box:
        return tile
    shape = tile.crop(box)
    target = CELL * 168 / 192
    k = min(target / shape.width, target / shape.height)
    shape = shape.resize((round(shape.width * k), round(shape.height * k)), Image.LANCZOS)
    out = Image.new("RGBA", tile.size, (0, 0, 0, 0))
    out.paste(shape, ((CELL - shape.width) // 2, (CELL - shape.height) // 2), shape)
    return out


def main():
    names = list(ICONS)
    rows = (len(names) + COLS - 1) // COLS
    tmp = "/tmp/solyra-icon-bake"
    os.makedirs(tmp, exist_ok=True)
    def source(n):
        path = ICONS[n]
        if path.startswith("local:"):
            return os.path.join(OUT, "src", path[len("local:"):])
        return os.path.join(THEME, path)

    imgs = "".join(
        f'<img src="file://{os.path.realpath(source(n))}" width={CELL} height={CELL}>'
        for n in names
    )
    html = (
        '<html><body style="margin:0;background:transparent">'
        f'<div style="display:grid;grid-template-columns:repeat({COLS},{CELL}px);'
        f'grid-auto-rows:{CELL}px">{imgs}</div></body></html>'
    )
    page, shot = f"{tmp}/sheet.html", f"{tmp}/sheet.png"
    with open(page, "w") as f:
        f.write(html)
    try:
        subprocess.run(
            [
                "google-chrome", "--headless=new", "--no-sandbox", "--disable-gpu",
                "--hide-scrollbars", "--allow-file-access-from-files",
                "--default-background-color=00000000",
                f"--window-size={COLS * CELL},{rows * CELL}",
                f"--screenshot={shot}", f"file://{page}",
            ],
            timeout=40,
            capture_output=True,
        )
    except subprocess.TimeoutExpired:
        # Chrome writes the screenshot and then lingers; the file is what we need.
        subprocess.run(["pkill", "-f", "headless"])
    sheet = Image.open(shot).convert("RGBA")
    for i, name in enumerate(names):
        r, c = divmod(i, COLS)
        tile = sheet.crop((c * CELL, r * CELL, (c + 1) * CELL, (r + 1) * CELL))
        if name in FIT_TO_BODY:
            tile = fit_to_body(tile)
        tile.resize((FINAL, FINAL), Image.LANCZOS).save(
            os.path.join(OUT, f"{name}.png"), optimize=True
        )
    with open(os.path.join(OUT, "manifest.json"), "w") as f:
        json.dump({"theme": THEME, "icons": ICONS}, f, indent=2)


if __name__ == "__main__":
    main()
