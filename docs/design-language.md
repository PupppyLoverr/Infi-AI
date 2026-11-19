# Solyra design specification

One idea: **a calm, warm-dark desktop where colour means state, chrome
disappears, and everything is reachable from the keyboard.**

This document is the build reference. Every number in it is either implemented
in `base/solyra-config.kdl` and `shell/panel`, or marked **planned**. If a value
here disagrees with the code, the code is wrong.

---

## 1. Principles

1. **The accent is a state, never decoration.** One accent element per region.
   Everything structural is greyscale.
2. **Chrome earns its pixels.** A surface exists to inform or to act. Nothing
   floats "for depth".
3. **Keyboard first.** Every action has a binding. The pointer is an
   alternative, never the only path.
4. **No fake affordances.** A control that cannot work does not ship. Pills read
   real workspace state; the dock reads real windows.
5. **Restartable.** The shell is a client. Killing it must never lose windows,
   workspaces, or the session.

---

## 2. Grid and spacing

A 4 px base grid. Every gap, inset, and size is a multiple of 4.

| Token | Value | Used for |
| --- | --- | --- |
| `space-1` | 4 | icon-to-label, pill padding |
| `space-2` | 8 | struts, card padding, cell gaps |
| `space-3` | 12 | popover padding, dock inset |
| `space-4` | 16 | bar content inset, section gaps |
| `space-6` | 24 | panel-to-content separation |

Corner radii: **7** pills, **10** cells, **12** cards and windows, **16** the
dock and the launcher, **14** the notch's bottom corners. Radii never mix
inside one surface.

---

## 3. Colour

Semantic roles first; values second. The light column is **planned** and exists
so nobody hardcodes a dark-only assumption.

| Role | Dark (shipped) | Light (planned) | Use |
| --- | --- | --- | --- |
| `bg` | `#121016` | `#F6F4F7` | Desktop, full-bleed surfaces |
| `surface` | `#1A1720` | `#FFFFFF` | Cards, popovers, windows |
| `surface-raised` | `#241F2A` | `#EDE9EF` | Inactive fills, pills |
| `hairline` | `#2A2430` | `#DCD6E2` | 1 px separators |
| `border-idle` | `#3A3342` | `#CFC8D4` | Unfocused window border |
| `accent` | `#B48AAD` | `#8A4F80` | The active thing, once per region |
| `on-accent` | `#141018` | `#FFFFFF` | Text/glyphs on accent |
| `text` | `#DCD6E2` | `#171319` | Primary text |
| `text-dim` | `#8B8394` | `#6B6472` | Secondary text only |
| `void` | `#07060A` | `#07060A` | The notch, always |
| `danger` | `#C4645C` | `#B23B33` | Destructive confirmation only |

Contrast: `text` on `bg` ≈ 11:1, `text-dim` on `bg` ≈ 4.6:1, `on-accent` on
`accent` ≈ 7:1. Dim text is never body copy.

---

## 4. Type

| Role | Face | Size | Weight | Notes |
| --- | --- | --- | --- | --- |
| Bar app name | Adwaita Sans | 12.5 | 600 | The focused app |
| Bar menus | Adwaita Sans | 12 | 400 | `text-dim` until hovered |
| Workspace digits | Adwaita Sans | 11 | 500 | Tabular |
| Clock | Adwaita Sans | 13 | 500 | Tabular figures |
| Date / status | Adwaita Sans | 11.5 | 400 | `text-dim` |
| Island text | Adwaita Sans | 11.5 | 500 | One line, ellipsised |
| Card titles | Adwaita Sans | 13 | 600 | |
| Card body | Adwaita Sans | 12.5 | 400 | `text-dim` |
| Key chords | DejaVu Sans Mono | 12 | 400 | Never proportional |

Both faces ship in the image. Inter is the intended replacement later; it
changes the face, not the scale.

---

## 5. Materials and depth

There is **no backdrop blur** — niri has no blur protocol, and pretending
otherwise produces muddy translucency. Depth comes from three things instead:

1. **Value steps.** `bg` → `surface` → `surface-raised`, each 8–12 lightness
   points apart.
2. **A 1 px top highlight** on floating cards (`#3A3344` at ~20 % alpha) so the
   card reads as lit from above.
3. **A hairline shadow** under floating cards: 2 px, `#000000` at 45 %.

Gradients are vertical and subtle: at most 8 lightness points across a card.
The bar is `#18141D` → `#121016`; the dock card is `#26212E` → `#19161F`.

---

## 6. Surfaces

### 6.1 Bar — **shipped**

32 px, full width, exclusive zone 32. Windows tile below it and never enter it.
The bar never hides, on any workspace.

Anatomy, left to right:

| Region | Content | Metrics |
| --- | --- | --- |
| Brand | 8 px accent dot + "Solyra" | dot at x=16, label at x=28, baseline 21 |
| Menus | `File Edit View Go Window Help` (**planned**) | 12 px, 12 px gaps |
| Workspaces | Pills 1–6, active filled accent | 18 × 14, radius 7, 6 px gap, starts at x=96 |
| Notch | Reserved void, fake lens | 186 × 26, radius 14 bottom, centred |
| Island | Grows out of the notch | 24 px tall, y=4, radius 12 |
| Status | Network, volume, battery (**planned**) | 11.5 px, right-aligned before the clock |
| Clock | `HH:MM` + `Wed 18 Nov` | 13 px, right inset 16 |

Hover on a pill raises it to `surface-raised`; the active pill stays accent.
Click sends `FocusWorkspace` to the compositor.

### 6.2 Island — **shipped**

The only surface allowed to interrupt the bar. One item at a time, most urgent
first.

| State | Content | Size |
| --- | --- | --- |
| Idle | Nothing; collapsed into the notch | — |
| Working | Accent dot + owner + verb | height 24, width = text + 46, max 420 |
| Attention (**planned**) | Icon + question + `⏎ approve` / `esc skip` | same, plus two key hints |

Source: `solyra-panel notify "text"` writes a timestamped line to
`$XDG_RUNTIME_DIR/solyra-island`; the panel shows it for 8 s, then collapses.
Not a notification centre, not a media player.

### 6.3 Dock — **shipped** (hybrid)

Floating card, bottom-centre, no exclusive zone. It floats over content rather
than reserving a strip of every workspace.

| Part | Metrics |
| --- | --- |
| Card | height 56 (8 px inset → 48 visible), radius 16, 20 px horizontal padding |
| Start cell | 34 × 34, radius 10, accent fill, `on-accent` glyph |
| App cell | 34 × 34, radius 10, `surface-raised`; hover → `#3A3245` + 2 px accent top edge |
| Running mark | 12 × 2 accent bar, 6 px above the card's bottom edge |
| Cell gap | 6 |
| Separator | 1 px `hairline`, 4 px vertical inset (**planned**, before the shelf) |
| Agent shelf | Cells per live agent session; hidden when none (**planned**) |

Behaviour: one cell per app, not per window. Click focuses the most recent
window; a second click cycles. Pinned apps that are not running appear after
running ones and launch on click. The card sizes to its content.

### 6.4 Launcher — **planned**

A Solyra surface replacing Fuzzel. Centred card, 16 radius, 640 wide, type-first:
input row, then results (icon, name, subtitle). The first result is selected;
`⏎` launches, `esc` dismisses, arrows move. No grid, no "all apps" wall — search
is the interface.

### 6.5 Control centre — **planned**

Anchored under the bar's status cluster. Rows: network, volume (slider),
battery, theme, and a power row. Each row is one line with a value on the
right; toggles use the accent track. 12 px radius, 320 wide.

### 6.6 Notifications — **planned**

Top-right stack, 12 px radius cards, 320 wide, app mark + title + one line of
body. Max three visible; the rest collapse into a count. The island carries
anything that needs a decision.

### 6.7 Window chrome — **shipped**

12 px corner radius, 2 px border: `accent` when focused, `border-idle`
otherwise. No titlebar furniture of our own; the client draws its own. Gaps and
struts come from the compositor: 8 px struts, 16 px gaps.

### 6.8 Overview — **shipped** (niri)

Zoomed-out workspaces on a dimmed backdrop. Keyboard: `Mod+Tab` toggles,
arrows move, `⏎` enters. No chrome of our own.

### 6.9 Lock — **planned**

Full-bleed `bg`, centred clock at 48 px, date under it, and a single password
field. No wallpaper, no widgets: a lock screen shows time and asks for a
password.

### 6.10 Setup wizard — **planned**

See `docs/setup.md`. Visual rules: one question per screen, the primary action
bottom-right, `esc` always skips, and a progress dot row at the bottom.

---

## 7. Motion

All `ease-out-expo`. Nothing blocks input.

| Transition | Duration |
| --- | --- |
| Window open | 180 ms |
| Window close | 150 ms |
| Window move | 120 ms |
| Island expand / collapse (**planned**) | 220 / 180 ms |
| Dock hover (**shipped**, instant tint) | 90 ms |
| Launcher open (**planned**) | 160 ms |

Over 200 ms needs a reason in the commit that adds it. Reduced motion maps
every duration to zero.

---

## 8. Interaction rules

1. **One way to do a thing.** The dock and the bar never duplicate a control.
2. **Click = focus, click again = cycle.** No surprise launches from a cell
   that is already running.
3. **Escape always dismisses** the topmost transient surface.
4. **Nothing is modal** except the lock screen and the setup wizard.
5. **Destructive actions** use `danger` and require a second confirmation.

---

## 9. Accessibility

- Focus is always visible: the 2 px accent border, never a colour-only change.
- Minimum target 24 × 24; dock cells are 34 × 34.
- Text scales with the compositor's scale factor; the bar grows with it.
- Reduced motion, and no animation on the lock screen.
- The shell never relies on colour alone: running apps also carry the
  underline, the active workspace also carries the filled pill.

---

## 10. Status

| Surface | State |
| --- | --- |
| Bar, notch, island, dock, window chrome, overview | **shipped** |
| Menus, status cluster, control centre, launcher, notifications, lock, setup | planned |

Planned surfaces have their metrics fixed here so they land consistent with
what ships, rather than being designed twice.
