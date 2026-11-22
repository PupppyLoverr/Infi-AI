# Bundled app icons

Dock and launcher icons for the apps the shell itself knows about. They are
embedded in the panel binary (`src/icons.rs`).

Source: the [WhiteSur icon theme](https://github.com/vinceliuice/WhiteSur-icon-theme)
by vinceliuice, licensed GPL-3.0 (this crate is GPL-3.0-only as well). Each PNG
is a 192 px render of one SVG from that theme with the squircle body inset
12 px; `bake.py` regenerates them and `manifest.json` records which SVG feeds
which name.

`browser` is first-party art (`src/browser.svg`, a plain globe), and `files` is
WhiteSur's generic folder: the theme's Finder and Safari lookalikes are left out
on purpose.

These are a stand-in for first-party Solyra artwork. Replace a file with a new
192 px RGBA PNG on the same canvas (body inset 12 px) and the panel picks it up
on the next build.
