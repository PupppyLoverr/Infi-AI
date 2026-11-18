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
- Notch: 186 × 26 px, centred, 14 px bottom corners, always void.
- Island: 24 px pill, 4 px below the top edge, expands from the notch.
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
| Dock | **Shipped** (hybrid) | Floating card, start cell, one cell per app, agent shelf when agents exist |
| Control centre | Planned | Network, volume, battery, theme, power |
| Notifications | Planned | Top-right stack, 12 px cards, per-app mark |
| Lock | Upstream swaylock | Solyra lock surface in P6 |
| Setup | Planned | First-boot wizard, see `docs/setup.md` |

## Bar, notch and island

The bar is permanent. It is an exclusive zone in the compositor, so tiled
windows start below it and can never enter it — no overlap, no hiding, no
"the bar slides away" state. It shows the focused app's name and menus on the
left, status on the right, and a reserved void in the middle.

**The notch.** The centre 186 × 26 px is drawn as a void — a black shape with
14 px bottom corners, with a fake lens dot, because hardware without a camera
still gets the same geometry. Nothing renders inside it. Reserving it now means
a laptop with a real notch looks identical to one without, and the shell never
has to reflow around hardware.

**The island.** The dynamic island is a 24 px pill that sits 4 px below the top
edge and grows out of the notch. It is the only surface allowed to interrupt
the bar. It holds one thing at a time, the most urgent:

| State | Shows | Example |
| --- | --- | --- |
| Idle | Nothing — collapsed to the notch | — |
| Working | Spinner, owner, one verb | "Agent · tidying Downloads" |
| Attention | Icon, question, two keys | "Delete 12 files? ⏎ approve · esc skip" |

It is not a notification centre and not a media player by default. When two
things compete, the one that needs a decision wins; the other waits.

Implementation: the bar is the panel layer surface and the island is drawn
inside it, growing horizontally out of the notch. It is fed by
`solyra-panel notify "text"`, which writes a message with a timestamp to
`$XDG_RUNTIME_DIR/solyra-island`; the panel shows it for eight seconds. A
separate layer surface can take over later if the island needs to grow taller
than the bar.

## Dock variants

Three behaviours, one icon set, drawn side by side for a decision. None is
built yet.

| Variant | Geometry | Running state | Cost |
| --- | --- | --- | --- |
| 1 · macOS style | Floating card, clear of the bottom edge, magnify on hover | Dot under the icon | Magnify needs compositor-side scaling or heavy redraws |
| 2 · Windows style | Edge-to-edge bar welded to the bottom | Short accent underline | Costs full width; its tray duplicates the top bar |
| 3 · Solyra hybrid | Floating card with a start cell and an agent shelf | Accent underline, one cell per app | Needs the agent shelf wired to real sessions |

The hybrid is the recommendation: macOS geometry keeps the desktop visible and
the card can float over a tiled window without stealing a strip of every
workspace, while the Windows half — one cell per app, grouping, an underline
instead of a dot — is what makes it usable with twenty windows open. The agent
shelf is the part neither reference has, and it is where this distro's whole
premise shows up: running agents get a home that is not your tile layout.

Decision: **the hybrid is built** (`shell/panel`). Variants 1 and 2 stay
documented as alternatives — if the hybrid's agent shelf turns out to be noise,
falling back to variant 1 is a drawing change, not a rewrite.

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
