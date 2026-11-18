# Solyra design language

One idea: a calm, warm-dark desktop where the accent means *active*, the chrome
disappears, and every surface is reachable from the keyboard. Windows tile; the
shell frames them; nothing decorative ships.

The tokens below are the ones in the code today — `base/solyra-config.kdl` for
the compositor and `shell/panel/src/main.rs` for the first shell surface — so
this document describes the product, not an aspiration.

## Colour

| Token | Value | Use |
| --- | --- | --- |
| `bg` | `#121016` | Desktop, panel, and every full-bleed surface |
| `surface` | `#1A1720` | Cards and popovers above the desktop |
| `pill` | `#241F2A` | Inactive control fills, workspace pills |
| `hairline` | `#2A2430` | 1 px separators; the panel's bottom edge |
| `border-idle` | `#3A3342` | Unfocused window border |
| `accent` | `#B48AAD` | The active thing: focus ring, selected pill, progress |
| `on-accent` | `#141018` | Text and glyphs on accent fills |
| `text` | `#DCD6E2` | Primary text |
| `text-dim` | `#8B8394` | Secondary text, inactive labels |

Rules that keep it coherent:

- The accent is a **state**, not decoration. One accent element per region.
- Everything structural is greyscale. Colour only appears for state, app
  identity, and wallpapers.
- Text on accent is `on-accent`, never white: the mauve is light, white on it
  reads as a smudge.
- Contrast: `text` on `bg` is ~11:1, `text-dim` on `bg` ~4.6:1, `on-accent` on
  `accent` ~7:1. Dim text is for secondary labels only, never for body copy.

## Type

| Role | Face | Size | Weight |
| --- | --- | --- | --- |
| Bar and control labels | Adwaita Sans | 11–11.5 | regular |
| Clock, window titles | Adwaita Sans | 13 | regular |
| Section headers | Adwaita Sans | 11 | semibold, `text-dim` |
| Key chords, terminal | DejaVu Sans Mono | 12 | regular |

Adwaita Sans and DejaVu Sans Mono both ship in the image, so the shell has no
font dependency of its own. Inter remains the intended replacement when the
first-party apps land; it changes the face, not the scale.

## Metrics

- Panel: 32 px tall, full width, exclusive zone 32. Content inset 16 px.
- Window: 12 px corner radius, 2 px border; `accent` when focused,
  `border-idle` otherwise.
- Workspace pill: 18 × 14, radius 7, 6 px gap. Active pill is `accent` with
  `on-accent` digits.
- Struts: 8 px on every edge, so tiled windows never touch the screen edge.
- Popovers: 12 px radius, 1 px `hairline`, shadow `softness 40`.

## Motion

From `base/solyra-config.kdl`, all `ease-out-expo`:

| Transition | Duration |
| --- | --- |
| Window open | 180 ms |
| Window close | 150 ms |
| Window move | 120 ms |

Motion is short and never blocks input. Anything longer than 200 ms needs a
reason in the commit that adds it. A reduced-motion setting will map to zero
durations when the settings app exists.

## Surfaces

| Surface | State | Intent |
| --- | --- | --- |
| Panel (top) | **Shipped** (P3 spike) | Brand mark, workspace pills, clock. Status cluster comes next. |
| Window chrome | **Shipped** (compositor) | Rounded corners, focus ring, no titlebar furniture of our own |
| Overview | **Shipped** (niri) | Zoomed-out workspaces, keyboard-driven |
| Launcher | Upstream Fuzzel | Becomes a Solyra surface with search-or-ask in P3/P4 |
| Dock | Planned | Bottom, floating, pinned plus running; opens the launcher |
| Control centre | Planned | Network, volume, battery, theme, power |
| Notifications | Planned | Top-right stack, 12 px cards, per-app mark |
| Lock | Upstream swaylock | Solyra lock surface in P6 |
| Setup | Planned | First-boot wizard, see `docs/setup.md` |

## Interaction rules

1. **Keyboard first.** Every action has a binding; the mouse is an alternative,
   never the only path. `Mod+Shift+Slash` shows the map.
2. **One way to do a thing.** The dock and the panel never duplicate a control.
3. **No fake affordances.** If a control cannot work yet, it does not ship.
   The panel's pills read real compositor state; when they cannot, they say so.
4. **Nothing floats over content without a reason.** Layer surfaces reserve
   space or dismiss on Escape; there is no permanent overlay.
5. **Restartable.** The shell is a client. Killing it must never lose windows,
   workspaces, or the session.

## Upstream apps, for now

Foot, Thunar, Fuzzel, and swaylock ship until their Solyra replacements are
stable. They are themed through the compositor where possible (borders,
corners, focus) and left alone where not — a half-themed GTK app looks worse
than an honest one. The Default Apps switcher in `docs/apps.md` is how a user
moves between the two.

## Light mode

Planned, not shipped. The token names stay; the values invert to a warm paper
palette with the same accent. Nothing in the shell may hardcode a colour
outside the token table, which is why the panel keeps its palette in one block
at the top of `main.rs`.
