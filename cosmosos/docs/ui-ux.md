# Cosmos UI/UX — a Windows + macOS blend

Design goal: borrow the best interaction ideas from Windows 11 and macOS and fuse them into one coherent, restrained desktop. One blue system accent reserved for true active state (Cosmos brand), plus colour where it is traditional — app-icon tints and the traffic lights. No decoration for decoration's sake — every element either informs or acts.

## Colour system

- **Accent** — `draw::accent(dark)` (`#3D8BFF` dark / `#0A6EE8` light) and `draw::accent_soft` (the same blue at ~25% alpha for selection washes behind text). Used only where state is genuinely *on*: toggle track, slider progress, mute pill, active-workspace pill, dock focus dot, launcher/assist selection, switcher cell, start glyph. egui apps get the same blue for text selection, hyperlinks, and the pressed widget (`uitk/theme.rs`).
- **Icon tints** — `icons::tint_for(app_id)`: Finder-blue Files, amber Editor, green Monitor, grey Settings, neutral Terminal. Applied wherever an *app* icon renders (dock, launcher grid + results, snap assist, switcher, notifications, menubar focused-app icon). System icons (wifi/volume/battery/power) stay neutral — they're status, not identity.
- **Traffic lights** — real macOS colours when focused (`DISC_COLORS` in `ssd.rs`: `#FF5F57`/`#FEBC2E`/`#28C840`), muted rings unfocused, near-black glyphs revealed on hover.
- **Wallpaper** — procedural aurora: deep-navy base + gaussian indigo/teal/violet blooms + vignette (`background_element` in `render.rs`), computed into a bilinear texture, no image asset. Light mode = pale slate + pastel blooms.
- Everything else stays greyscale: cards, menubar, panel, borders, text.

## Language mix per surface

| Surface | From macOS | From Windows 11 | Cosmos choice |
|---|---|---|---|
| **Dock (bottom)** | Floating centred translucent card, icon strip, running dots, pinned/running divider, no exclusive zone (windows slide under) | Centred taskbar arrangement, click-to-focus/launch, mirrors ALL running apps | New surface: start glyph + pinned icons + running extras — the Dock IS the taskbar |
| **Panel (top)** | Menu-bar idiom: app mark + **focused app name** (semibold) left, system status right | Workspace pager | Pure menubar — task buttons moved to the dock; the two-bar split is the signature blend |
| **Window chrome** | Traffic-light cluster (left, circles, glyph-on-hover), centred title, 32px, **rounded top corners** | — | macOS chrome fully incl. real red/yellow/green discs when focused; 10px corner radius |
| **Launcher** | Spotlight: centred floating card, dim backdrop, type-first; Launchpad icon grid | Start: search field, "PINNED" grid, "ALL APPS" list, footer with system actions | Spotlight placement + Start contents; typing flips grid → Spotlight results |
| **Tray** | — | Quick Settings flyout on tray click | Flyout card anchored to tray: network, volume slider, battery, power row |
| **Workspace pager** | — | Win11 virtual-desktop strip | Numbered pills; active = accent pill with white digits |
| **Notifications** | Banner stack top-right, app icon per card | Toasts top-right | Kept; rounded 12px cards, per-app icon, drop shadow |
| **Background** | Soft tonal field | — | Procedural aurora: navy base + indigo/teal/violet gaussian blooms + vignette |
| **Shadows** | Popover/menubar drop shadows | — | SDF gaussian decals under every floating card (dock, flyout, notifications, switcher, launcher, assist) — the compositor paints them for bounded layer surfaces, the shell paints them for fullscreen overlays |
| **Snap preview** | — | Snap-assist zone hint | Opaque 2px hairline outline of the drop zone (solid fills can't alpha-blend on llvmpipe; the outline reads cleaner anyway) |
| **Corners** | Rounded cards | Rounded everything | All shell cards unified at 12px radius (dock keeps 15px as the largest card); windows keep square bodies with rounded titlebar tops |

## Skipped (deliberately)

- **Start-logo bottom-left (Windows)**: the mark lives on the panel AND the dock's left cell — both open the launcher.
- **Rounded bottom window corners**: titlebar top corners are rounded (10px); bottom corners need per-client alpha shaping in the renderer — deferred.
- **Blur/translucency materials (Mica/Vibrancy)**: shm buffers + llvmpipe; translucency exists only as alpha-tinted fills on our own surfaces.
- **Snap flyout on maximize hover**: keyboard snapping (Super+arrows) already ships; flyout deferred — no fake affordances.
- **Animations**: honour `reduce_motion`; any motion added stays <150ms and positional.

## Type & metrics

- Panel (menubar) 32px; titlebar 32px; dock height 54 (8px float margin), cell 46, icon 28, card radius 15.
- All other shell cards: 12px radius (launcher, quick settings, notifications, window switcher, snap-assist picker).
- Launcher: Spotlight vignette scrim (airy near the card, deeper at corners) + card shadow; grid 6 columns, cell 78px high, icon 30 + 10.5 label; section headers semibold 11px; footer buttons carry the settings/logout icons.
- Shadows: gaussian SDF pixmap — window chrome + layer decals 32px bleed, σ≈11, α≈0.36, +10px down; in-canvas card shadows 20px bleed, σ≈9, α≈0.34, +7px — cached per card size.
- Title font 13; menubar app name 13 semibold; panel text 12–13; flyout body 13 / headers 11 dim.
- Traffic-light circles: 12px diameter, 20px pitch, order L→R = close, minimize, maximize. Unfocused = grey rings; focused = macOS-coloured discs; glyphs appear on hover only.
- Icon set (`shell/src/icons.rs`): monochrome stroke glyphs on a 24px grid — start mark, terminal, files, editor, settings, monitor, lock, logout, reboot, shutdown, generic window — shared by panel, dock, launcher, flyout and notifications.
- Snap Assist input region covers only the dimmed free half below the menubar: tray/dock/snapped-window clicks pass through (the picker dismisses on the resulting focus loss) instead of being swallowed.
- Hover parity: launcher pinned cells / RECOMMENDED rows / footer buttons highlight under the pointer (`launcher_hover`), cleared on Leave/close; dock separators: after the start glyph and between pinned apps and running extras.
- Icon-led flyout rows: Network/Volume/Battery each carry their tray glyph at the left edge; the mute pill shows the speaker icon, not letters.
