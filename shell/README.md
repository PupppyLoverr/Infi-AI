# solyra-shell

The shell is what the compositor is not: bars, docks, launchers, notifications,
and the control centre. It runs as ordinary Wayland clients on
`zwlr_layer_shell_v1` surfaces, so `solyra-comp` stays a compositor and the
shell can be restarted without taking the session down.

## What exists

One binary, `solyra-panel`, owns three surfaces:

- **Bar** — 32 px, top, exclusive zone 32. Brand, workspace pills, a reserved
  notch in the centre, clock and date on the right. Windows tile below it and
  can never enter it.
- **Island** — drawn inside the bar, growing horizontally out of the notch.
  `solyra-panel notify "Agent · tidying Downloads"` puts a message there for
  eight seconds; it collapses when the message expires.
- **Dock** — the hybrid from `docs/design-language.md`: a floating card with
  an accent start cell, one cell per running app, and pinned apps that are not
  running. Running cells carry an accent underline.

Everything is drawn with a software buffer (`wl_shm`): background, hairlines,
the notch shape, rounded pills and cells, and text rasterised with `ab_glyph`
from Adwaita Sans. No C dependencies; the binary is about 1.1 MB.

**Input is real.** The panel binds a seat and a pointer:

| Click | Effect |
| --- | --- |
| A workspace pill | `niri msg action focus-workspace N` over the IPC socket |
| A dock cell for a running app | focuses that window |
| A dock cell for a pinned app | launches it |
| The start cell | opens the launcher (`fuzzel`) |

The shell holds no state the compositor does not: workspace pills, running
apps and window ids are all read from niri's IPC socket, refreshed once a
second.

Also in the panel: a launcher with grouped, ranked results (apps, windows,
commands, and an explicit "Ask an agent" row), a control centre that shows only
state it can read (link state, battery if present, memory), and an agent tasks
card with approvals driven by `docs/agent-protocol.md`
(`solyra-panel toggle-agents`, `Mod+Shift+A`).

Not built yet: app menus in the bar, a status cluster (network, volume, battery),
the control centre, notifications beyond the island, the window switcher, and
the agent shelf in the dock (it stays hidden until agent sessions exist).

## Build

```sh
sudo docker build -f comp/dev.Containerfile -t solyra-dev:dev comp/
sudo docker run --rm -v "$PWD:/repo" -w /repo/shell/panel solyra-dev:dev \
  sh -ec 'cargo build --locked --release &&
          install -m 755 target/release/solyra-panel ../out/solyra-panel'
```

`Cargo.lock` is committed and the build uses the same Fedora 46 toolchain as
the compositor, so the binary matches the image's glibc. The image ships it at
`/usr/bin/solyra-panel`.

## Run it

Inside a session that already has `solyra-comp` running:

```sh
solyra-panel &                        # bar + dock
solyra-panel notify "Build finished"  # a message in the island
```

The panel finds niri's socket from `NIRI_SOCKET`, or the newest
`niri.wayland-*.sock` in `XDG_RUNTIME_DIR`. It does not start at login yet:
there is no display manager, and the shell still has no status cluster.

## Test it

```sh
cd shell/panel
cargo test --locked                      # logic, protocol, contrast, render smoke test
SOLYRA_RENDER_DIR=/tmp/preview cargo test --locked preview   # also writes PNGs
```

The PNGs are flat-backdrop renders of each surface at scale 1, with no compositor
blur: for layout and contrast review, not a screenshot of the desktop.

## Verify it

`.amp/in`-scratch harness, not shipped: Xvfb gives a real X seat, weston's X11
backend forwards that seat to a nested niri, and `xdotool` moves the pointer.
Assertions read niri's IPC, so they prove the click changed the compositor
rather than just repainting the panel.

```sh
bash .amp/in/orb-backup/shell-test.sh
# workspace before click: [1] after pill click: [2]
# windows after dock click: ['thunar', 'foot']
# PASS workspace pill switched
# PASS dock cell launched Files
```

The same harness produced the two captures used in the progress report: the
dock with two running apps, and the island showing a message.

## Space reservation

Measured in a nested session at 1280x800 against the shipping config:

| State | Window top edge | Window height |
| --- | --- | --- |
| No panel | 24 px | 745 px |
| Panel, exclusive zone 32 | 56 px | 719 px |
| Panel, exclusive zone 64 (temporary) | 88 px | 680 px |

The top edge moves by exactly the exclusive zone, which is the property that
matters: the compositor reserves the bar's space and tiled windows start below
it. The dock has no exclusive zone, so it floats over content like the macOS
dock rather than stealing a strip of every workspace.
