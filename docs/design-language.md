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

Corner radii: **7** pills, **10** cells, **12** cards and windows, **18** the
floating dock, **16** the launcher, **14** the notch's bottom corners. Radii
never mix inside one surface.

---

## 3. Colour

Semantic roles first; values second. The light column is **planned** and exists
so nobody hardcodes a dark-only assumption.

| Role | Dark (shipped) | Light (shipped) | Use |
| --- | --- | --- | --- |
| `bg` | `#1C1C1E` | `#F2F2F7` | Desktop chrome (neutral graphite) |
| `surface` | `#1C1C1E` @ ~77% | `#F2F2F7` @ ~82% | Cards, popovers — translucent for Glass |
| `bar` | `#1C1C1E` @ ~44% | `#F2F2F7` @ ~50% | Menu bar and dock fills |
| `hairline` | white @ 14% | black @ 11% | 1 px separators |
| `accent` | `#0A84FF` | `#007AFF` | Active control (system blue) |
| `on-accent` | `#FFFFFF` | `#FFFFFF` | Text/glyphs on accent |
| `text` | `#F5F5F7` | `#1C1C1E` | Primary text |
| `text-dim` | `#C7C7CC` | `#48484E` | Secondary text only |
| `void` | `#000000` | `#000000` | The notch, always |
| `danger` | `#FF453A` | `#FF3B30` | Destructive confirmation only |

Default wallpaper is **Beach** (Unsplash photograph). Default accent is **Blue**.
Mauve remains available in Settings but is no longer the product default.

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

The bar, the dock and the notch each choose a material in Settings >
Appearance (`bar_material`, `dock_material`, `notch_material` in
`shell.conf`). The panel paints the tint, rim and lit edge; solyra-comp
(our niri fork, which has backdrop blur) draws what only the compositor can
see. The panel asks for blur on exactly the painted shape with
`ext-background-effect-v1`, so the clear room above a magnifying dock or
around the island is never blurred.

| Material | Panel paints | Compositor adds | Use |
| --- | --- | --- | --- |
| **Solid** | opaque fill, faint rim | nothing | maximum legibility; the classic black notch |
| **See-through** | light tint (alpha ~150), rim | nothing; the desktop stays sharp | minimal looks |
| **Glass** (default bar, dock) | near-clear tint (alpha 44 dark / 60 light), soft rim, 1 px lit top edge | blur, saturation x1.5 | the macOS-like look |
| **Acrylic** | tint mixed 22-30 % with the wallpaper's average colour (alpha ~190) | blur, saturation x1.15, grain 0.06 | the Windows-like look |

**Adaptive density.** See-through gets denser (alpha +70) when the wallpaper's
lightness fights the text colour. Glass gets a smaller lift (alpha +52) so a
bright wallpaper still reads through the menu bar and dock: a bright wallpaper
under light-on-dark text, or the reverse. This is the legibility guard that both
Apple (Liquid Glass adapts to what is behind it) and Microsoft (Acrylic's
luminosity layer) build in. **Solid surfaces everywhere** (`opaque=1`)
overrides every material, as an accessibility switch.

Apps: zgx apps open transparent windows over the compositor's window blur.
The sidebar or tab strip is the most see-through layer (alpha 150), the
content area denser (214), cards a faint wash; `app_glass=0` or Solid
surfaces makes them opaque. `solyra-ui::scope` maps zgx's theme tokens to
Solyra's, so every stock control uses the Solyra palette and the chosen
accent.

Previews: `SOLYRA_RENDER_DIR=out cargo test` in `shell/panel` writes
`material-*.png`, whole desktop scenes per material, dark and light. The
compositor's blur is approximated there.

---

## 6. Surfaces

### 6.1 Bar — **shipped**

26 px, full width, exclusive zone 26. Styles (`bar_style`): **Menu bar**
(Finder-style: a small Solyra mark, the focused app's name in semibold, then
Go, Window, View, Help in `text-dim` until hovered, each a real dropdown),
**Compact** (workspaces and status only) and **Hidden** (no strip, nothing
reserved). Right side, unboxed glyphs: search, network, volume, battery when
present, control centre, then the date and the clock as plain text (date
then time; 12/24 h, seconds and date optional). Workspaces, when shown, are
quiet digits with a dot on the active one.

Menus open on a full-screen transparent overlay: a click outside closes,
hovering another title switches, as on macOS. Restart and Shut down ask for
a second click.

### 6.2 Notch and Dynamic Island — **shipped**

One black (or chosen material) shape at the top centre that morphs between
idle, a compact live activity and an expanded card, frame by frame (instant
with reduced motion). `notch_style`: **Notch** (hangs from the screen edge,
concave ears), **Dynamic Island** (a floating capsule) or **Off**.

Priority, most urgent first: an agent approval or question (approve, deny,
answer inline; keys A/D/T), a fresh notification, what is playing (any MPRIS
player, with play/pause/next/previous), an agent at work. Expanded idle shows
a glance: time, date, battery, network, running agents.

### 6.3 Dock — **shipped** (four variants)

`dock_style`: **Solyra** (floating card), **Floating** (macOS-like, optional
magnification), **Taskbar** (Windows-like full-width bar: task view, start,
a quiet search pill, centred apps, a grouped tray, clock) and **Minimal**
(icons only). The floating card is 52 px tall, radius 18, with 40 px cells,
a 5 px gap and 14 px padding, lifted 12 px off the bottom edge. Running apps
carry a small dot; a hairline separates the trash. Auto-hide
tucks the dock away 0.7 s after the pointer leaves; touching the bottom
edge brings it back. One cell per app; a click focuses the latest window or
launches the app.

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

### 6.9 Lock — **shipped**

`solyra-panel lock` (Super+Ctrl+L): ext-session-lock on every output,
password checked by PAM (`/etc/pam.d/solyra-lock`, same stack as login) off
the UI thread, input through xkb (any layout), Caps Lock shown, back-off
after repeated failures. `lock_style`: **Solyra** (blurred wallpaper, time
and field centred), **Centred** (macOS-like: date and time at the top, avatar
and field below), **Clock corner** (Windows-like: large clock lower left,
any key shows the sign-in card) and **Dashboard** (large date and clock,
nL or now-playing capsule, avatar row with a NEW circle, agents / events /
media glance). swaylock stays installed as the fallback
and recovery (Super+Alt+L).

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
- Minimum target 24 × 24; dock cells are 40 × 40.
- Text scales with the compositor's scale factor; the bar grows with it.
- Reduced motion, and no animation on the lock screen.
- The shell never relies on colour alone: running apps also carry a dot,
  the active workspace also carries a dot under its digit.

---

## 10. Implementation notes (verified in code)

- Surfaces are flat (one fill plus a hairline); the accent is `#B48AAD` (light
  `#8A4F80`) and marks only the active or selected thing. The app tiles are one
  flat style; real system icons are used when found. There is no notch.
- Colour, radii and type sizes live in `shell/panel/src/tokens.rs`; surfaces
  read a `Theme` (dark or light, `theme=` in `shell.conf`) and carry no colour
  literals except app-icon art and the always-black notch/island, which uses the
  dark palette in both themes. The text roles are unit-tested for WCAG AA
  (4.5:1) over both a black and a white backdrop.
- **Correction to §5:** the compositor config enables backdrop blur
  (`background-effect { blur true }`) and the shell paints glass assuming it.
  Treat §5's "no blur" as superseded; depth rules (value steps, rim, hairline)
  still apply when blur is unavailable.
- Agent approval uses an amber role, not the accent: "working" and "needs a
  decision" must not look alike. State is also carried by shape and label.
- Not supported yet: HiDPI/fractional scaling (surfaces render at scale 1),
  non-ASCII keyboard input in the launcher (raw evdev keys only), reduced
  motion (the shell has no animations to reduce), screen readers.

## 11. Status

| Surface | State |
| --- | --- |
| Menu bar, notch / island, four docks, materials, lock screen, launcher, control centre, notifications, agent tasks and approvals | **shipped**, verified by unit tests and rendered previews; not yet run on a live session by a person |
| zgx apps (Settings, Terminal, Browse, Setup) | **shipped**, screenshotted under headless weston in CI |
| Snap zones (niri is a scrolling tiler; presets instead), screen readers in the shell, fractional scaling | not done |
