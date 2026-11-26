# Solyra plan (macOS 27 "Golden Gate"-style distro)

Supersedes `docs/plan-2026-niri-fedora.md` and the Tauri + React shell.

## Architecture
- **Shell:** Arch Linux + KDE Plasma 6 Wayland, as pearOS does. Native
  QML/C++ for the dock, menu bar, global menu, notch, Spotlight (KRunner),
  Control Center, notifications, widgets, lock screen and decorations.
  Glass: fork of `pearOS-archlinux/liquid-gel` (GPL-3.0).
  *Gated on the Milestone 1 RAM and disk numbers (below).*
- **Terminal:** native Rust app on zgx (pinned rev `699ae02e`, already used by
  `apps/`), with engine logic ported from zeronsh/zeron (MIT): alacritty_terminal
  plus portable-pty. No GPUI, and no GPL code.

## Budgets (whole distro)
Idle RAM 100–150 MB, 2–3 GB installed, ISO < 3 GB, near-zero idle CPU.
Measured by `.github/workflows/measure.yml` (`tools/measure/`), because this
dev container has no KVM and no Arch mirror access: Arch cloud image,
2 vCPU / 4 GiB, virtio-gpu (llvmpipe), tty autologin into `startplasma-wayland`,
`free -m` 5 min after plasmashell starts, stock then trimmed.

## Audit (2026-11-26)
**This repo, working today:**
- Fedora bootc image (not Arch).
- niri fork with refraction glass and the `solyra_glass_v1` protocol.
- Rust layer-shell panel: bar, dock, notch, launcher, control, lock with PAM, widgets.
- zgx apps: Files, Settings, Browse, Setup.
- `web/` (Tauri): now superseded for the shell; its terminal design is the
  reference for the zgx terminal.

**pearOS-archlinux (26 repos):**

| Repo | Licence | Use |
|---|---|---|
| liquid-gel | GPL-3.0 | Glass effect: base for ours |
| pkgbuilds | none at the root | pearos-dock is GPL-2.0+ (WaveTask fork); pearos-window-borders is GPL-3; pearos-notch is "custom", so reference only |
| pearos-themesw, pearos-sounds, pafari, pearos-apps-bundle | GPL-3.0 | Reusable inside GPL components |
| pearos-settings, macos-keyboard-shortcuts-kde, pearos-icons, artwork, filesystem, installer, calamares config | none | Reference only (pearos-icons recreate Apple icons: never use) |

## Milestone 1 results (CI run 45813098984, 2026-11-26)
Setup: Arch cloud image, `plasma-desktop` + mesa + konsole (588 packages),
QEMU/KVM with 2 vCPU / 4 GiB, virtio-vga with llvmpipe, measured 5 min idle
after plasmashell started.

| | stock | trimmed |
|---|---|---|
| `free -m` used | **1192 MB** | **1144 MB** |
| Total RSS, all processes | 2094 MiB | 1981 MiB |
| Root fs used | 3024 MB | 3026 MB |
| Boot to graphical.target | 10.2 s | 9.3 s |
| plasmashell up at (uptime) | 15.3 s | 14.3 s |

- **Largest processes (trimmed):** plasmashell 517 MiB, kwin_wayland 305,
  kscreenlocker_greet 272, kded6 63, powerdevil 56, xdg-desktop-portal 54,
  ksecretd 51, kaccess 51, polkit agent 50, kactivitymanagerd 41,
  Xwayland 39 MiB.
- **What the trim did:** turned off baloo (file indexing). Nothing else
  applied: none of Discover, PackageKit, KDE Connect, browser integration,
  drkonqi, welcome, vault and similar were installed by `plasma-desktop`,
  and no bluetooth, cups, avahi, ModemManager or power-profiles services
  were enabled. So stock was already near-minimal.
- **Caveats:**
  - Plasma's default auto-lock fired during the 5 idle minutes, so
    kscreenlocker_greet (~0.3 GB) is in both numbers. That is real default
    behaviour.
  - llvmpipe inflates kwin and plasmashell compared with real GPU hardware.
  - Even without the locker and with a GPU, Plasma idles at roughly
    0.8–0.9 GB, about 6× the 150 MB target.
- **Verdict:** Plasma misses the RAM budget by a wide margin and sits at
  the top of the 3 GB disk budget before any pearOS components or apps.

## Decision gate
If trimmed Plasma misses the 150 MB / 3 GB budget, the user picks one:
- **(a)** Plasma + pearOS components with a realistic budget, or
- **(b)** a lightweight compositor with a custom shell. This is what Solyra is
  today: niri fork + Rust panel.

## Milestones
1. Audit and measure (this) → 2. zgx blur spike on KWin → 3. terminal core
→ 4. glass and tokens, transparency slider → 5. menu bar and dock
→ 6. Spotlight, notch, Control Center, notifications → 7. terminal polish
→ 8. ISO and VM boot → 9. extras (none listed yet).
