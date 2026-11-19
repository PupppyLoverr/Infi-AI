# First-boot setup (P6)

A new machine should feel like it was made for its owner within five minutes.
The flow borrows the shape people already know — Windows OOBE and the macOS
Setup Assistant — and keeps the Solyra difference: every step is skippable, and
skipping everything leaves a working desktop.

Nothing here is implemented yet. This is the spec P6 builds against.

## Flow

| # | Step | What it does | Skippable |
| --- | --- | --- | --- |
| 1 | Welcome | Language and keyboard layout, one screen | No (has a default) |
| 2 | Network | Pick a Wi-Fi network or continue offline | Yes |
| 3 | Account | Username, password, avatar; or keep the provisioned user | No |
| 4 | Look | Dark or light, accent colour, wallpaper from the shipped set | Yes |
| 5 | Apps | Optional bundles: **Gaming**, **Creative**, **Development** | Yes |
| 6 | Agents | Enable the agent platform, pick sandbox level 1 or 2, or off | Yes |
| 7 | Privacy | Diagnostics off by default; opt in per category | Yes |
| 8 | Ready | A short summary of what was chosen, then the desktop | — |

Order matters: network before apps (bundles need it), look before apps (so the
first launch is already themed), agents after privacy is explained.

## Gaming, specifically

The Gaming bundle is the answer to "can it play games" without shipping
anything we cannot license:

- **Steam and Proton come from Flatpak**, installed during setup from Flathub,
  not baked into the image. That keeps the image redistributable and the
  32-bit graphics runtime where it belongs.
- **gamescope**, **MangoHud**, and **gamemode** ship in the image: they are
  free, small, and useful for any game, not just Steam.
- Controller support is udev rules plus `steam-devices` equivalents, applied
  when the bundle is chosen.
- The bundle is reversible from Settings: removing it removes the Flatpaks and
  the rules, not the user's game data.
- Storage: the bundle is large (tens of GB with games). The setup screen says
  so before the download, with the free space on the target.

## Account

- The development image provisions `solyra` with no password. Setup is where a
  real account is created: username, password, and whether that account is an
  administrator.
- The provisioned development user is renamed or replaced; either way there is
  exactly one human account after setup.
- Password quality is checked with `pwquality`, not a bespoke rules dialog.

## Agents

- Off by default. Turning it on creates the `solyra-agent` sandbox user and
  writes the default policy, and says plainly what an agent can reach.
- Level 2 (per-agent VM) is offered only when the host has the resources, and
  the screen shows the cost before it is chosen.

## Technical shape

- `solyra-setup.service`, `Type=oneshot`, ordered after `network.target` and
  before the display manager (which does not exist yet — see below).
- The service runs a full-screen client on tty1. The first version is a
  keyboard-driven TUI; the zgx version replaces it when the shell can host it.
  Both write the same files, so the TUI is not throwaway.
- Completion writes `/var/lib/solyra/setup-done` with the choices, and the
  service disables itself. `solyra-setup --reset` re-runs it.
- Every step has a non-interactive equivalent (`--language`, `--account`,
  `--bundle gaming`) so a VM or an image build can preseed a machine without
  clicking through.

## What is missing before this can ship

- A display manager or a first-boot session handoff. Today nothing starts a
  session at boot (base/README.md records this), so setup has no desktop to
  hand over to.
- A settings app to revisit these choices later (`docs/apps.md`).
- A Flatpak-capable image: `flatpak` is not installed yet, and Flathub must be
  configured as a remote at first use, not baked in.


## Status of what exists (experimental)

- `solyra-setup` (zgx wizard) collects a name, user name, password and computer
  name, validates them, and writes a mode-0600 request file.
- `solyra-setup-apply` (root; run by `solyra-setup-apply.service`, which only
  runs while `/var/lib/solyra/setup-done` is absent) validates the request
  again, refuses links and loosely-permissioned files, deletes the request
  before acting, runs `useradd`, `chpasswd` (password on stdin, never in argv)
  and `hostnamectl`, and writes the done marker only if all succeeded.
  Both are unit-tested; the helper's refusals are exercised with `--dry-run`.
- **Not wired in:** nothing starts the wizard on a fresh install yet. That needs
  a login flow for the very first boot (greetd would have to autologin a
  temporary setup user only until setup is done), which is not built. The
  development disk skips setup entirely (it creates `setup-done`).
- `solyra-install /dev/DISK` wraps `bootc install to-disk` with disk checks and
  a typed confirmation. It has not been run end to end, and there is no ISO.
