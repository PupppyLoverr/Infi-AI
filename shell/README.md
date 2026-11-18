# solyra-shell

The shell is what the compositor is not: bars, docks, launchers, notifications,
and the control centre. It runs as ordinary Wayland clients on
`zwlr_layer_shell_v1` surfaces, so `solyra-comp` stays a compositor and the
shell can be restarted without taking the session down.

## What exists

`solyra-panel` is the first surface and the P3 spike the plan asked for: a
32 px top bar that reserves screen space, draws the Solyra palette, reads
workspace state from niri's IPC socket, and keeps a live clock.

- Anchored top/left/right, exclusive zone 32, keyboard interactivity `none`.
- Workspace pills come from `niri msg --json workspaces`, so they show what the
  compositor has rather than a mock.
- The clock repaints on a periodic `wl_display.sync` wake-up; no second event
  loop, no polling thread touching the queue.
- Drawn with a software buffer (`wl_shm`): background, hairline, dot, rounded
  pills, and text rasterised with `ab_glyph` from Adwaita Sans.
- A reserved notch in the centre: 186 × 26, square at the top edge, 14 px
  bottom corners, a fake lens dot. Nothing renders inside it, so hardware with
  and without a camera looks identical. The dynamic island will grow out of it
  as its own layer surface.

Not built yet: dock, launcher, control centre, notifications, window switcher,
and the `zgx` rendering host. See PLAN.md §4 (P3) and
`docs/design-language.md` for what each surface should be.

## Build

```sh
sudo docker build -f comp/dev.Containerfile -t solyra-dev:dev comp/
sudo docker run --rm -v "$PWD/shell/panel:/src" -w /src solyra-dev:dev \
  cargo build --release
```

The binary lands at `shell/panel/target/release/solyra-panel` (about 1.1 MB,
stripped). `Cargo.lock` is committed; builds use the same Fedora 46 toolchain
as the compositor so the binary matches the image's glibc.

## Run it

Inside a session that already has `solyra-comp` running:

```sh
WAYLAND_DISPLAY=wayland-1 solyra-panel
```

The panel finds niri's socket from `NIRI_SOCKET`, or the newest
`niri.wayland-*.sock` in `XDG_RUNTIME_DIR`.

## Verify the space reservation

`.amp`-scratch test, not shipped: weston (headless) → `solyra-comp` nested →
panel, then compare the tiled window's geometry before and after the panel
appears. With the shipping config at 1280x800:

| State | Window top edge | Window height |
| --- | --- | --- |
| No panel | 24 px | 745 px |
| Panel, exclusive zone 32 | 56 px | 719 px |
| Panel, exclusive zone 64 (temporary) | 88 px | 680 px |

The top edge moves down by exactly the exclusive zone in both cases, which is
the property that matters: the compositor reserves the bar's space and tiled
windows start below it. The height delta is not exactly the zone because the
bottom edge also shifts with the scrollable layout's gaps.
