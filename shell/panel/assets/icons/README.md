# Bundled app icons

Dock, launcher and app icons for the apps and places the shell knows about.
They are embedded in the panel binary (`src/icons.rs`) and in Solyra apps
(`apps/solyra-ui/src/icons.rs`).

These are Solyra's own drawings, made by `make_icons.py` (no icon library or
third-party artwork). The style follows macOS: a superellipse squircle body
with a vertical gradient, one bold glyph, a lit top edge and a soft shadow.
Each PNG is 192 px with the body inset 12 px (the panel relies on that 168/192
ratio, see `icons::BODY`); the folder and trash are free-standing shapes.

    python3 make_icons.py    # redraws every PNG here

## Using your own icons

Put PNGs in `~/.local/share/solyra/icons/` and the dock, Spotlight and the
taskbar use them instead of the bundled set. Name each file after a role or
an application id:

| File | Replaces |
| --- | --- |
| `files.png` | Files (the first dock icon) |
| `launchpad.png` | Launchpad |
| `browser.png` | the web browser (or `org.mozilla.firefox.png` for Firefox only) |
| `terminal.png`, `editor.png`, `viewer.png`, `calculator.png`, `settings.png` | those apps |
| `downloads.png`, `trash.png` | the right end of the dock |

Any size works (large icons are shrunk once when first shown). macOS-style
icons with a transparent margin are measured, so their body lines up with the
other icons. Restart the panel (log out and in) after adding files.

These icons stay on your machine. Solyra does not ship icons from third-party
icon sites: most of those are other companies' app artwork, which Solyra has
no licence to redistribute.
