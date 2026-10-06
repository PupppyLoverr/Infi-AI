# Cosmos UI/UX — a Windows + macOS blend

Design goal: borrow the best interaction ideas from Windows 11 and macOS and fuse them into one coherent, restrained, monochrome desktop. No accent colours (Cosmos brand), no decoration for decoration's sake — every element either informs or acts.

## Language mix per surface

| Surface | From macOS | From Windows 11 | Cosmos choice |
|---|---|---|---|
| **Dock (bottom)** | Floating centred translucent card, icon strip, running dots, no exclusive zone (windows slide under) | Centred taskbar arrangement, click-to-focus/launch | New surface: start glyph + pinned icons + running extras — the Dock IS the taskbar |
| **Panel (top)** | Menu-bar idiom: app mark + **focused app name** (semibold) left, system status right | Workspace pager | Pure menubar — task buttons moved to the dock; the two-bar split is the signature blend |
| **Window chrome** | Traffic-light cluster (left, circles, glyph-on-hover), centred title, 32px, **rounded top corners** | — | macOS chrome fully; monochrome circles; 10px corner radius |
| **Launcher** | Spotlight: centred floating card, dim backdrop, type-first; Launchpad icon grid | Start: search field, "PINNED" grid, "ALL APPS" list, footer with system actions | Spotlight placement + Start contents; typing flips grid → Spotlight results |
| **Tray** | — | Quick Settings flyout on tray click | Flyout card anchored to tray: network, volume slider, battery, power row |
| **Workspace pager** | — | Win11 virtual-desktop strip | Numbered pills; active = inverted pill |
| **Notifications** | Banner stack top-right | Toasts top-right | Kept; rounded cards, 8px radius |
| **Background** | Soft tonal gradient | — | Subtle vertical monochrome gradient (no artwork) |
| **Corners** | Rounded cards | Rounded everything | Rounded shell surfaces (launcher, flyouts, notifications); windows stay square — see below |

## Skipped (deliberately)

- **Start-logo bottom-left (Windows)**: the mark lives on the panel AND the dock's left cell — both open the launcher.
- **Rounded bottom window corners**: titlebar top corners are rounded (10px); bottom corners need per-client alpha shaping in the renderer — deferred.
- **Blur/translucency materials (Mica/Vibrancy)**: shm buffers + llvmpipe; translucency exists only as alpha-tinted fills on our own surfaces.
- **Snap flyout on maximize hover**: keyboard snapping (Super+arrows) already ships; flyout deferred — no fake affordances.
- **Animations**: honour `reduce_motion`; any motion added stays <150ms and positional.

## Type & metrics

- Panel (menubar) 32px; titlebar 32px; dock height 54 (8px float margin), cell 46, icon 28, card radius 15.
- Launcher card radius 14; grid 6 columns, cell 78px high, icon 30 + 10.5 label.
- Title font 13; menubar app name 13 semibold; panel text 12–13; flyout body 13 / headers 11 dim.
- Traffic-light circles: 12px diameter, 20px pitch, order L→R = close, minimize, maximize. Unfocused = outlines; focused = discs; hover = glyph inside.
- Icon set (`shell/src/icons.rs`): monochrome stroke glyphs on a 24px grid — start mark, terminal, files, editor, settings, monitor, lock, logout, reboot, shutdown, generic window — shared by panel, dock and launcher.
