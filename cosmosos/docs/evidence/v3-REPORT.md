# v3 verification report — devin/1791460645-v3-system-menu @880af4a

Real OVMF boot @1920×1080 (greetd → greeter → cosmos session), driven via
serial shell + cosmos-ipc + QEMU monitor screendumps. Two sessions: the first
ended via IPC `quit_session` after the lock-brick investigation; captures span
both.

## Check sheet

| # | Check | Verdict | Evidence |
|---|-------|---------|----------|
| 1 | No dark strip x≈0–200; dock pill keeps soft shadow | PASS | v3-01 — dock is a floating pill, soft shadow, no left strip |
| 2 | App bodies violet-tinted (#1C1730-ish) in dark; light in light mode | **FAIL** | v3-03 — Files/Editor bodies render light-grey under `appearance=dark`; bodies follow neither dark tokens nor the flip (same as earlier rounds) |
| 3 | `accent_rgb` in config.json matches wallpaper accent; ocean → blue accents | PASS (IPC) / **FAIL (Settings path)** | `accent_rgb:[30,141,216]` persisted after `set_config wallpaper ocean`; serial `wallpaper accent extracted rgb=[30, 141, 216] wallpaper=ocean`. But the Settings wallpaper thumbnail row moves its selection ring and never propagates — compositor config stayed `violet` |
| 4 | CC + launcher 14px corners; no glass banding | PASS | v3-06 — rounded corners consistent; menubar/dock glass smooth |
| 5 | `journalctl -b \| grep -i panic` empty; idle RAM in budget | **FAIL (1 panic)** | teardown panic on `quit_session` (below); RAM 562 MiB used / 1404 avail with 4 apps — inside budget |

## Feature checks

| Item | Verdict | Evidence |
|------|---------|----------|
| Workspace dots in menubar (#133) | PASS | v3-01/02 — dots render, active workspace tinted |
| Clock format `Thu 8 Oct 22:14` | PASS | menubar reads `Thu 8 Oct 12:15` |
| App menus File/Window/Help (#134) | PASS | v3-14 — glass dropdown, items work |
| Window menu → Tile Window to Left | PASS | Files → `x:74,y:36,w:917,h:1040` (left half) via list_windows |
| Window menu → Minimize | PASS | `minimized:true`, `output:""` — left the space |
| Esc closes open menu | PASS | dropdown gone on next frame |
| Cosmos-mark system menu | PASS (opens) | v3-15 — Settings…/Lock Screen/Log Out…/Restart…/Shut Down… |
| System menu → Lock Screen | **FAIL** | `WARN cosmos_shell: menu: sys.lock failed: org.freedesktop.DBus.Error.ServiceUnknown: The name org.freedesktop.login1 was not provided by any .service files` — wayland session isn't logind-registered (`loginctl list-sessions` shows only the serial tty) |
| super+L → lock card | **FAIL** | cosmos-lock spawns, `cosmos: lock surface committed`, but the card renders invisible (wallpaper only, v3-16). Blind-typing the password doesn't unlock; killing the client leaves the compositor holding the lock and a second client is refused → session unrecoverable without quit |
| Files file-type icons + kind labels (#130) | PASS | v3-04 — Archive/Source code/Text/Image icons + Type column |
| Dotfiles hidden | PASS | `.hidden-dot` absent, count consistent |
| Settings wallpaper thumbnail row | Renders / **apply broken** | v3-12 — 6 thumbs in 2×3, ring selects, no propagation (above) |
| `-light` wallpaper variants (#131) | PASS | v3-02 — pastel ocean-light under `appearance=light` |
| Terminal ANSI palette, light (#132) | PASS | v3-13 — `printf '\e[3%dm colour%d'` rows 0–7 all distinct + `ls --color` |
| xdg-open file association (bonus) | PASS | opening `archive.tar.gz` spawned firefox-esr with the file URI |

## Panic (verbatim, teardown path — session 1 quit_session)

```
cosmos-session[551]: ipc: quit session requested
cosmos-session[551]: Io error: Broken pipe (os error 32)
cosmos-session[551]: thread 'main' (566) panicked at src/server/mod.rs:802:21:
cosmos-session[551]: Failed flushing clientside events: Io(Os { code: 32, ... "Broken pipe" })
cosmos-session[551]: ERROR drm_atomic: smithay::backend::drm::device::atomic: Failed to restore previous state. Error: Permission denied (os error 13)
cosmos-session[551]: cosmos-session: compositor exited (0) — logging out (session restarts on respawn)
```

Also: `cosmos-lock[1758]: pam_unix(cosmos-lock:account): setuid failed: Operation not permitted` — a manually launched cosmos-lock can't PAM-auth (only the compositor-spawned instance has privilege context).

## Greeter respawn — PASS

`quit_session` → greetd respawned the greeter on vt1 (v3-17): blurred wallpaper,
large `12:14 / Thursday, October 8` clock, initial avatar `C`. New logind
session registered (`New session 5 of user cosmos`).

## Standing owner-side defects (carried forward)

1. App bodies ignore `appearance` entirely (render light under dark).
2. cosmos-lock card invisible; session unrecoverable once locked.
3. System-menu Lock Screen uses the logind path but the wayland session is not
   registered → item silently fails.
4. Settings wallpaper picker: ring moves, no compositor propagation.
5. quit_session teardown panics flushing client events (broken pipe).
6. Top-layer press-resync works for menus/dropdowns (verified this round) —
   still unverified on island/approval/quick-settings surfaces.
