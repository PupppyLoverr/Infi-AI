# Cosmos UI/UX — a Windows + macOS blend

Design goal: borrow the best interaction ideas from Windows 11 and macOS and fuse them into one coherent, restrained, monochrome desktop. No accent colours (Cosmos brand), no decoration for decoration's sake — every element either informs or acts.

## Language mix per surface

| Surface | From macOS | From Windows 11 | Cosmos choice |
|---|---|---|---|
| **Panel** | Top menu-bar position, 32px, system status right | Taskbar job: app task buttons, workspace pager | Top bar that IS the taskbar — one surface, not two |
| **Window chrome** | Traffic-light cluster (left, circles, glyph-on-hover), centred title, 32px | — | macOS chrome fully; monochrome circles |
| **Launcher** | Spotlight: centred floating card, dim backdrop, type-first | Start: footer strip with system actions + Settings | Spotlight layout + a Start-style footer |
| **Tray** | — | Quick Settings flyout on tray click | Flyout card anchored to tray: network, volume slider, battery, power row |
| **Workspace pager** | — | Win11 virtual-desktop strip | Numbered pills; active = inverted pill |
| **Notifications** | Banner stack top-right | Toasts top-right | Kept; rounded cards, 8px radius |
| **Background** | Soft tonal gradient | — | Subtle vertical monochrome gradient (no artwork) |
| **Corners** | Rounded cards | Rounded everything | Rounded shell surfaces (launcher, flyouts, notifications); windows stay square — see below |

## Skipped (deliberately)

- **Dock (macOS)**: the panel already carries the taskbar role; a dock duplicates it.
- **Start-logo bottom-left (Windows)**: same reason; panel is top.
- **Rounded window corners**: needs per-client alpha shaping in the renderer — deferred; the shell surfaces that we paint ourselves (launcher, flyouts, notifications) get radius where it's cheap and visible.
- **Blur/translucency materials (Mica/Vibrancy)**: shm buffers + llvmpipe; translucency exists only as alpha-tinted fills on our own surfaces.
- **Snap flyout on maximize hover**: keyboard snapping (Super+arrows) already ships; flyout deferred — no fake affordances.
- **Animations**: honour `reduce_motion`; any motion added stays <150ms and positional.

## Type & metrics

- Panel 32px; titlebar 32px; launcher card radius 12; flyout radius 10; chips radius 6.
- Title font 13; panel text 12–13; flyout body 13 / headers 11 dim.
- Traffic-light circles: 12px diameter, 20px pitch, order L→R = close, minimize, maximize. Unfocused = outlines; focused = discs; hover = glyph inside.
