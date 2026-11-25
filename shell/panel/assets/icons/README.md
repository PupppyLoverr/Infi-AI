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
