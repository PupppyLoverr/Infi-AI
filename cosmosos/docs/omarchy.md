# CosmosOS vs Omarchy

[Omarchy](https://omarchy.org/) (DHH) is an opinionated Arch setup: Hyprland
compositor + Quickshell widgets + curated TUIs + theming, assembled on top of
Arch. CosmosOS is more custom, not less Linux: Debian trixie userspace plus a
desktop stack written **from scratch** in Rust — our own Smithay compositor,
our own layer-shell, our own egui toolkit, our own apps.

| Aspect            | Omarchy                              | CosmosOS                              |
|-------------------|--------------------------------------|---------------------------------------|
| Base              | Arch                                 | Debian trixie (live/bootable image)   |
| Compositor        | Hyprland (C++)                       | cosmos-compositor (Rust, Smithay)     |
| Shell/UX          | Quickshell + DHH dotfiles            | cosmos-shell: dock, menubar, launcher,|
|                   |                                      | quick settings, Snap Assist, switcher |
| Window model      | Tiling-first                         | Floating default + super+T master/    |
|                   |                                      | stack tiling per workspace + edge snap|
| Terminal          | foot/alacritty (external)            | cosmos-terminal: vt100 + SGR mouse +  |
|                   |                                      | 256/truecolor, per-cell renderer      |
| Dev tooling       | mise, docker, lazygit, curated TUIs  | opencode agent + curated TUI set      |
| Theming           | Theme packs                          | Dark/light pill + 6 accent presets    |
|                   |                                      | (azure/ember/forest/violet/rose/mono) |
| Install           | `omarchy` installer script           | `image/build.sh` reproducible image   |

## What we took from Omarchy

- **Agentic-by-default**: `opencode` (upstream standalone binary) is baked
  into the image and one click from the launcher (`cosmos-terminal -e`).
- **Curated terminal life**: ripgrep, fd, fzf, eza, bat, btop, fastfetch,
  neovim, tmux, lazygit, htop, jq, tree — plus a tuned `.bashrc` (colored
  prompt, `eza`/`bat` aliases, `EDITOR=nvim`, `checkwinsize`).
- **Keyboard-first bindings**: `Super+Return`→terminal, `Super+Space`→launcher,
  `Super+E`→files, `Super+Tab`/`Alt+Tab`→switcher, `Super+M/Q/F/D`,
  `Super+T` per-workspace tiling, `Super+1-9` workspaces,
  `Super+Shift+1-9` move-to-workspace, `Super+arrows` snap.
- **Help overlay**: super+? opens a grouped keybind cheatsheet card
  (Omarchy's discoverability pattern), driven by one `KEYBINDS` table.
- **Whole-OS theme dial**: 6 accent presets re-render wallpaper blooms,
  toggles, pills, icons and selections live from one config key.
- **Real typefaces**: Inter for UI text, JetBrains Mono for key chords
  and terminal — the same face choices Omarchy ships.
- **Opinionated defaults**: nothing optional is half-installed — what's on
  the image is on the launcher, themed, and working.

## What we deliberately don't take

- **Tiling-by-default** — tiling exists (super+T master+stack, per
  workspace) but stays opt-in: the floating Windows/macOS blend is the
  default window model, not a grid-first world.
- **Arch + AUR churn** — Debian base for stability; the image is reproducible
  from `image/build.sh` with pinned sysctls and a hardened default posture
  (see `security.md`).
- **External compositor** — Hyprland is someone else's DE. Everything above
  the kernel here is Cosmos code, which is what makes the blend possible.
