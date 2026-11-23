# Bundled app icons

Dock and launcher icons for the apps the shell itself knows about. They are
embedded in the panel binary (`src/icons.rs`).

These are **first-party Solyra artwork** — a cohesive set of gradient
squircle tiles with simple flat glyphs, drawn for the distro and licensed
with it. No third-party icon theme ships in the image. Third-party apps
that are not in the set (Firefox, Chrome, ...) fall back to the system
icon theme — their own real icons — or, when nothing resolves, to a
neutral tile.

Canvas rules: each PNG is 192 px RGBA with the squircle body inset 12 px
on every side (the panel scales by `1 / icons::BODY`); corners are
transparent. `manifest.json` records what each name depicts.

To replace an icon: drop a new 192 px RGBA PNG on the same canvas.
