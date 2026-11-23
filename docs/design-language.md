# Solyra design specification

**Version 2 — the Perihelion refresh.** One idea: a calm, warm-dark desktop
where colour means state, chrome disappears, and the agent layer is a
first-class citizen — borrowing the best of macOS (the bar, the dock, the
materials) and Windows 11 (the task density, the snap model, the control
centre) into something that is unmistakably Solyra.

This document is the build reference. Every value is either implemented in
`base/solyra-config.kdl`, `crates/solyra-tokens` and `shell/panel`, or marked
**planned**. If the code disagrees, the code is wrong — fix it.

**Originality rule.** Every icon, wallpaper, mark, glyph and sound in the
image is drawn or composed for Solyra. Third-party lookalike themes, icons,
fonts, wallpapers and artwork are never shipped — no Apple's, Microsoft's,
anyone else's. Names like Finder, Dock, Spotlight, Dynamic Island, Mica and
Acrylic describe *influences* in these documents only; they never appear in
the product UI. Influences end at ideas; what we ship is ours.

---

## 1. Principles

1. **The accent is a state, never decoration.** One accent element per
   region. Everything structural is greyscale.
2. **Chrome earns its pixels.** A surface exists to inform or to act.
   Nothing floats "for depth".
3. **Keyboard first.** Every action has a binding; the pointer is the
   alternative, never the only path.
4. **No fake affordances.** A control that cannot act on the real system
   does not ship. Pills read real workspace state; the dock reads real
   windows; toggles are wired or absent.
5. **Restartable.** The shell is a client: killing it must never lose
   windows, workspaces or the session.
6. **Honest agent UI.** Approvals show the exact action, resource and
   reversibility; missing fields render as "not reported", never as a
   comfortable default. The panel never claims an agent exists when none
   is connected.

---

## 2. The layout

One canonical desktop, not a theme soup:

- **Top:** the menu bar — full width, 26 px, exclusive. Left: the `nL`
  mark (plain text in the bar's ink, no box), the focused app's name in
  semibold, then the menus (Go, Window, View, Help). Right: search,
  network, volume, battery, control centre, date, clock.
- **Centre:** the workspace, scrollable tiling by default.
- **Bottom:** the floating dock — a centred glass card: pinned apps,
  running dots, launcher cell, separator, trash.
- **Live area:** the notch hanging from the top edge (or the floating
  island capsule) carries what needs the person: agent approvals and
  questions, notifications, now playing.

Everything else — Compact or Hidden bar, the Solyra card and Taskbar and
Minimal docks, the four lock layouts, light theme — is one switch away in
Settings. The defaults are the product.

---

## 3. Grid, radii, spacing

4 px base grid; every gap, inset and size is a multiple of 4.

| Token | px | Used for |
| --- | --- | --- |
| `space-1` | 4 | icon-to-label, pill padding |
| `space-2` | 8 | struts, card padding, cell gaps |
| `space-3` | 12 | popover padding, dock inset |
| `space-4` | 16 | bar content inset, section gaps |
| `space-6` | 24 | panel-to-content separation |

Radii (`crates/solyra-tokens`): `R_CONTROL` 8 (pills, chips),
`R_ROW` 10 (menu rows, list cells), `R_TILE` 14 (cards, windows),
`R_CARD` 20 (launcher results card), `R_OVERLAY` 22 (agent desk),
dock card 19, notch bottom corners 14. Radii never mix inside one surface.

---

## 4. Colour

Semantic roles first, values second (`crates/solyra-tokens`).

| Role | Dark | Light | Use |
| --- | --- | --- | --- |
| `surface` | `#1C1C1E` @ 77% | `#F2F2F7` @ 82% | overlay cards (launcher, control centre, island card) |
| `bar` | `#1C1C1E` @ 44% | `#F2F2F7` @ 50% | menu bar and dock fills |
| `hairline` | white @ 14% | black @ 11% | 1 px separators |
| `field` | black @ 44% | white @ 86% | recessed inputs, slider tracks |
| `tile` / `tile_hover` | white @ 6% / 12% | black @ 4% / 7% | idle and hovered controls |
| `text` | `#F5F5F7` | `#1C1C1E` | primary text |
| `text_med` | `#F2F2F7` @ 91% | `#3A3A3C` @ 90% | secondary-but-still-body text |
| `text_dim` | `#C7C7CC` | `#48484E` | secondary text only, never body copy |
| `accent` | `#006AE0` | `#005CD0` | the active/selected thing only |
| `on_accent` | `#FFFFFF` | `#FFFFFF` | text and glyphs on accent |
| `void` | `#000000` | `#000000` | the notch, always |
| `danger` | `#FF453A` | `#C40012` | destructive confirmation only |
| `success` | `#32D74B` | `#187430` | ok, done |
| `warning` | `#FFD60A` | `#9A6000` | attention that is not dangerous |
| `agent_active` | = accent | = accent | "an agent is working" |
| `agent_approval` | `#FF9F0A` | `#9A6000` | "an agent needs you" — deliberately **not** the accent |

Contrast floors (unit-tested): `text` and `text_med` ≥ 4.5:1 over both
themes' glass in its worst backdrop; `text_dim` ≥ 4.5:1; every accent's
`on_accent` ≥ 4.5:1. State colours are legible as text on glass.

Accents offered: Blue (default), Mauve, Teal, Green, Amber, Rose,
Graphite — each tuned per theme; `selected_fill`/`selected_border`
derive from it automatically.

Default wallpaper: **Beach** (original art, `assets/wallpapers/`).
Default accent: Blue. Default theme: Dark.

---

## 5. Type

Adwaita Sans everywhere; DejaVu Sans Mono for key chords. Inter remains
the intended face upgrade later — it changes the face, not the scale.

| Role | px | Weight |
| --- | --- | --- |
| Bar app name | 13.0 | 600 |
| Bar menus / status | 13.0 | 400 |
| Workspace digits | 11.5 | 500 (active 600) |
| Clock / date | 13.0 / 11.5 | 500 / 400, tabular figures |
| Island text | 11.5 | 500, one line ellipsised |
| Card titles | 14.0 | 600 |
| Body | 13.0 | 400 |
| Captions ("RESULTS") | 10.8 | 500, never body copy |
| Key chords | 12.0 | mono |

---

## 6. Materials

Four surface materials, per surface (`bar_material`, `dock_material`,
`notch_material` in `shell.conf`); "Solid surfaces" (`opaque=1`) is the
accessibility switch that overrides all of them.

| Material | Panel paints | Compositor adds | Feel |
| --- | --- | --- | --- |
| **Solid** | opaque fill, faint rim | nothing | maximum legibility |
| **See-through** | light tint, rim | nothing — desktop stays sharp | minimal |
| **Glass** *(default bar + dock)* | a near-neutral white veil with a lit directional rim | blur, slight saturation lift | the airy, luminous default |
| **Acrylic** | tint mixed 24–30% toward the wallpaper's average colour, soft rim | blur, saturation, fine grain | the dense, wallpaper-tinted one |

Rules that keep it honest:

- Glass is a *white* veil, never grey — a grey veil turns warm wallpaper
  mauve.
- **Adaptive density:** See-through densifies (alpha +70) when the
  wallpaper fights the ink; on Glass, the ink itself flips light/dark
  when the wallpaper demands it (the automatic-menu-bar behaviour), so
  no scrim is needed.
- Blur is requested per shape via `ext-background-effect-v1` on exactly
  the painted shape — the clear room above a magnifying dock or around
  the island is never blurred.
- zgx apps open transparent windows over the compositor's window blur:
  sidebar alpha ~59%, content ~84%, `app_glass=0` or Solid makes them
  opaque. `solyra-ui::scope` maps zgx theme tokens onto these.

Previews: `SOLYRA_RENDER_DIR=out cargo test` in `shell/panel` writes
whole-desktop PNGs per material and theme.

---

## 7. Surfaces

### 7.1 Menu bar — **shipped**

26 px, full width, exclusive zone 26. Three styles: **Menu bar**
(default — the Finder-style layout below), **Compact** (workspaces +
status only), **Hidden** (nothing reserved).

- **Left:** the `nL` mark — plain text in the bar's ink (white ink on
  dark chrome, dark ink on light; no box, no tile). In Menu bar it opens
  the Solyra menu (About, System Settings, Lock, Log Out, Restart,
  Shut Down — the last two confirm); in Compact it opens the launcher.
  Then the focused app's name in semibold, then Go, Window, View, Help —
  real dropdowns on a full-screen overlay: click-outside closes, hover
  switches, every item acts (niri IPC, `solyra-open`, power).
- **Right:** search, network (Wi-Fi arcs / wired bar / struck-through
  offline), volume (level arcs), battery when present, control centre,
  then date and clock as plain text (12/24 h, optional seconds/date).
- **Workspaces** (optional): quiet digits, a 1.4 px accent dot under the
  active one, click switches.

### 7.2 Notch and Island — **shipped**

One pure-black (`void`, always, both themes) shape at top centre that
morphs idle → compact live activity → expanded card, frame by frame
(instant under reduced motion). Styles: **Notch** (hangs from the edge,
concave ears), **Island** (floating capsule), **Off** (the live
activity moves into a black pill in the bar itself).

Priority, most urgent first:

1. An **agent approval or question** — amber pill, approve/deny/answer
   inline (keys A/D/T), exact action + resource + reversibility.
2. A fresh **notification**.
3. **Now playing** — any MPRIS player, play/pause/next/prev.
4. An **agent working** — accent dot, click opens the agent card.

Expanded idle: a glance — time, date, battery, network, running agents.

### 7.3 Dock — **shipped**, four variants

**Floating** is the default (the product look): a centred glass card,
56 px tall, radius 19, 42 px cells, 7 px gap, 12 px padding, 12 px off
the bottom edge — apps, the launcher cell, a separator, the trash.
Running apps carry a quiet dot; click = focus latest window, click again
= cycle. Optional magnification (icons grow up into unblurred space).

Also: **Solyra** (a floating card with start cell + agent shelf),
**Taskbar** (full-width 48 px bar: start mark, frosted search pill,
centred apps with underlines, grouped tray, clock), **Minimal** (icons
only, smaller). **Auto-hide** for all: tucks away 0.7 s after the
pointer leaves; the bottom edge brings it back.

Config names are Solyra's own: `floating`, `taskbar`, `solyra`,
`minimal` (old `macos`/`windows` config values still parse, silently).

### 7.4 Launcher — **shipped**

A floating search capsule (580×56, fully rounded) about a seventh down
the screen, placeholder "Search or Ask". An empty query is only the
capsule — nothing lists until there is something to match, and `⏎` does
nothing. A typed query opens a results card (24 px radius, 10 px below):
ranked groups (Top Hit, applications, open windows, commands, then an
agent row), first result selected. Rows are 40 px, one line: icon,
title, dimmer subtitle; the selection is a flat accent pill with `↵`.
`esc` dismisses.

The capsule and card are two shapes on one transparent surface the panel
resizes to fit; the panel draws the shadow and asks the compositor to
blur only behind those two shapes.

### 7.5 Control centre — **shipped**

Anchored under the bar's status cluster, 348 px, grouped rows of
controls that all act on the real system: network readout, volume and
brightness sliders, theme and material switches, power row. Nothing that
does nothing.

### 7.6 Notifications — **shipped**

Top-right stack, radius-12 cards, app mark + title + one line; max three
visible, the rest collapse into a count. Focus/DND holds back plain
notifications — agent approvals are never muted. The island carries
anything needing a decision.

### 7.7 Window chrome — **shipped**

12 px corner radius, 2 px border: accent when focused, `border-idle`
otherwise. No titlebar furniture; the client draws its own. Compositor
struts 8 px, gaps 16 px.

### 7.8 Overview — **shipped** (niri)

Zoomed-out workspaces on a dimmed backdrop; `Mod+Tab` toggles, arrows
move, `⏎` enters.

### 7.9 Snap Assist — **planned** (P2 gate)

Windows-11-style zones: drag to an edge/corner, the zone previews in a
glass capsule, release to snap; remaining windows offer themselves in a
snap-assist card. Zones are a compositor feature (`comp/`), not a panel
one. Until it ships, layout presets + floating toggles cover the need.

### 7.10 Lock — **shipped**

`solyra-panel lock` (Super+Ctrl+L): ext-session-lock on every output,
PAM auth off the UI thread, any xkb layout, Caps Lock indicator,
back-off after failures. Layouts: **Solyra** (blurred wallpaper, centred
time + field), **Centred** (macOS-like), **Clock corner** (Windows-like),
**Dashboard** (big clock, avatar row, agents/events/media glance).
swaylock stays installed as recovery (Super+Alt+L).

### 7.11 Agent desk — **shipped**

`desktop_mode=agent` puts a 360×392 glass widget board at top right:
date/clock, workspace card, Bento tiles for running agents and target
environment, Cloud/Local/Worktree segmented control.

### 7.12 Setup wizard — **planned**

`docs/setup.md`: one question per screen, primary action bottom-right,
`esc` always skips, progress dots at the bottom.

---

## 8. Motion

All `ease-out-expo`; nothing blocks input.

| Transition | ms |
| --- | --- |
| Window open / close / move | 180 / 150 / 120 |
| Island expand / collapse | 220 / 180 |
| Dock hover tint | 90 |
| Launcher open | 160 |

Over 200 ms needs a reason in the commit. Reduced motion maps every
duration to zero.

---

## 9. Interaction rules

1. **One way to do a thing.** The dock and the bar never duplicate a
   control.
2. **Click = focus, click again = cycle.** No surprise launches.
3. **`esc` always dismisses** the topmost transient surface.
4. **Nothing is modal** except the lock screen and the setup wizard.
5. **Destructive actions** use `danger` and need a second click.

---

## 10. Accessibility

- Focus always visible: the 2 px accent border, never colour alone.
- Minimum target 24×24; dock cells 40×40.
- Text scales with the compositor scale; the bar grows with it.
- Reduced motion; no animation on the lock screen.
- State is never colour-only: running apps carry a dot, the active
  workspace carries a dot, offline strikes the network glyph.
- Still missing (honest): screen readers in the shell, fractional
  scaling.

---

## 11. Identity

- **Mark:** `nL` — always plain text in the surface's ink colour. Never
  boxed, never a logo lockup, never a third-party glyph.
- **Icons:** a bundled 192 px set on the squircle canvas (body inset
  12 px), drawn for Solyra — see `shell/panel/assets/icons/README.md`.
  Anything unbundled falls back to the system icon theme on a neutral
  tile.
- **Wallpapers:** original art in `assets/wallpapers/` (1080p ships,
  4K sources kept).
- **Fonts:** Adwaita Sans, DejaVu Sans Mono (OFL-family licences).
- **The notch is always `void` black**, in both themes — hardware, not
  chrome.

---

## 12. Status

| Surface | State |
| --- | --- |
| Menu bar, notch/island, four docks, materials, lock, launcher, control centre, notifications, agent desk, agent tasks + approvals | **shipped** — unit-tested and preview-rendered; awaiting a live-session run |
| zgx apps (Settings, Terminal, Browse, Setup) | **shipped** — CI self-tests under headless Wayland |
| Snap zones + Snap Assist | compositor work, P2 gate — **not started** |
| Screen readers in shell, fractional scaling | not done |
