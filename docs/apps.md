# First-party apps (P4)

The rule that governs this stage: **upstream apps ship until a Solyra app is
demonstrably better, and both stay installed.** A user who prefers Thunar keeps
Thunar; nothing is removed on day one. This is a product decision and also the
cheapest insurance while zgx is at 0.1.

## Inventory

| Role | Original (always installed) | Solyra app (zgx) | Status |
| --- | --- | --- | --- |
| Terminal | Foot | `solyra-term` | Built: tabs, clear regression tests. Self-tested in CI; not yet driven by a person in a VM |
| Browser | Firefox | `solyra-browse` | Built over zgx's WebKitGTK view: tabs, address bar, bookmarks, history, downloads. Same caveat |
| Settings | the system tools (`nmtui`, `pavucontrol`, ...) | `solyra-settings` | Built: appearance, background, sound, network, default apps, agents, display, about |
| Files | Thunar | `solyra-files` (default) | Built: Finder-style sidebar, icon and list views, search, rename, copy/cut/paste, new folders, trash/restore, recents, stars and colour tags. Same caveat |
| Launcher | Fuzzel | built into `solyra-panel` | Shipped |
| First-boot setup | none | `solyra-setup` | Experimental, not started by default (docs/setup.md) |

The originals are never removed. `solyra-open terminal|files|browser` (used by
Super+Return, Super+E, Super+B and the dock) reads Settings -> Default apps and
falls back to the original if the chosen app is missing or exits with an error
within three seconds. The image default for every role is the original; a
person switches a role to the Solyra app in Settings.

Foot, Thunar, Fuzzel, and swaylock are installed in `base/Containerfile`
today. There is no settings app yet, which is why the setup wizard in
`docs/setup.md` writes configuration directly.

## Replacement order

1. **Terminal.** Smallest surface with real daily use, and the best proof that
   zgx can host a text-heavy app. Scope for v1: PTY, scrollback, tabs, themes
   from the design tokens, selection and clipboard, 256-colour and truecolour,
   no image protocols, no ligatures. `alacritty_terminal` is the intended
   engine; the window is zgx.
2. **Files.** v1: list and grid, breadcrumbs, open with the default app,
   rename, delete with confirmation, search in the current folder, mount
   visibility. Privileged operations go through `xdg-desktop-portal`, never
   through a root helper of our own.
3. **Settings.** The first screen that must exist before anything can be
   "customisable af": appearance (accent, dark/light, wallpaper, motion),
   input (keyboard layout, tap-to-click), displays, Default Apps, agents, and
   about. Each page writes a file the rest of the system already reads —
   `comp.kdl` for the compositor, `solyra-defaults.toml` for roles.
4. **Monitor and editor** after the first three, since they are the least
   blocking.
5. **Browser** is Track A from PLAN.md: chrome in zgx over WebKitGTK. Not
   scheduled in this stage.

## Default Apps switcher

One file decides which app answers a role:

```toml
# /etc/solyra/defaults.toml (system) or ~/.config/solyra/defaults.toml (user)
[roles]
terminal = "foot.desktop"
files = "solyra-files.desktop"
```

- Both the upstream and the Solyra app install a desktop entry, so either can
  answer the role.
- The launcher shows both entries until a role is switched, then shows the
  chosen one first and the other under an "Also installed" group.
- Keybindings read the role, not a hardcoded binary: `Mod+Return` spawns the
  terminal role. Today `comp.kdl` hardcodes `foot`; this changes when the
  settings app exists, and the binding stays in one place.
- Switching is reversible in one click, which is the point.

## What "done" means for this stage

The daily-driver checklist, run on a real boot:

- [ ] Open a terminal, run a command, copy and paste, resize, scroll back.
- [ ] Open files, navigate two levels, rename a file, delete a file, cancel a delete.
- [ ] Switch the terminal role to the Solyra app and back without a logout.
- [ ] Change the accent colour and see it in the panel, focus ring, and pills.
- [ ] Every action above is also reachable from the keyboard alone.

Nothing in this list is checked yet. The first code is the terminal, and the
first commit for it should be a window that runs a shell and nothing else.
