# CosmosOS v4 — report

Tree: main `36b37d4`. Every PASS below comes from a real QEMU boot at 1920x1080
of the code at the SHA given. Screenshots are compositor IPC captures of that
boot. Raw logs: `v4-blockers.txt` (first A round) and `v4-retest.txt` (final round, image from
`9bf0ac2` = #150–#157 + #161, then #162 boot-checked on its own).

## A — blockers

| Item | Result | SHA | Evidence | Proof |
|---|---|---|---|---|
| A1 logind session | PASS | 20f282d (#148) | v4-retest.txt | `loginctl` Type=wayland Class=user via greetd + pam_systemd; `loginctl lock-session` locks |
| A2 lock / wrong pw | PASS | 20f282d | v4-14-lock.png, v4-15-lock-wrong-password.png | wrong password → "Incorrect password", session stays locked |
| A2 locker death → takeover | PASS | 20f282d | v4-16a-lock-respawned.png, v4-16-lock-respawn-unlocked.png | kill -9 locker → `respawned locker pid=803 took over`, unlock through it |
| A3 no panics | PASS | 36b37d4 | v4-18-cc-logout.png, v4-19-greeter-after-logout.png | `journalctl -b \| grep -i panic` = 0 lines (1239 journal lines), incl. logout → greeter |
| A4 live wallpaper/accent | PASS | 6d9d582 (#151) | v4-07-settings-ocean-accent.png, v4-17-ocean.png | Ocean click → wallpaper, blur, accent, shell and apps update in ~1s |
| A5 Files rows = footer | PASS | 20f282d | v4-03-files-etc.png | checker `rows=0 footer=0 ls=0`; /etc "126 items, 3 hidden" |
| A6 terminal RSS t5→t30 | PASS | 15729f2 (#155) | v4-retest.txt | VmRSS 25824 kB at t5/t10/t15/t20/t25/t30 → +0.0% (strict <10%) |

## B — cosmos-kit

| Item | Result | SHA | Evidence | Proof |
|---|---|---|---|---|
| Kit components + gallery, both themes | PASS | 55dc9a2 (#149) | kit-gallery-dark.png, kit-gallery-light.png, kit-gallery.txt | every component and icon drawn by `cosmos-kit-gallery` on a booted image |
| Popover / Sheet / Dropdown open | PASS | #159 | kit-gallery-popover.png, kit-gallery-sheet.png, kit-gallery-dropdown.png | overlays open and stay open; 3/3 headless click tests |
| Narrow tables, disabled contrast | PASS | #159 | v4-03-files-list-dark.png | Name keeps ≥160px, lower-priority columns dropped |
| Lint | PASS | 36b37d4 | — | `cargo clippy -p cosmos-kit -p cosmos-files -p cosmos-kit-gallery -- -D warnings` exit 0 (x86_64 and aarch64) |

## C — apps on the kit

| App | Result | SHA | Evidence | Proof |
|---|---|---|---|---|
| Files | PASS | 4e04def (#150) | v4-03-files-list-dark.png, v4-04-files-grid-light.png, v4-03-files-etc.png | Ctrl+L → Go to Folder → /etc; list + grid; Trash/New Folder |
| Settings | PASS | 6d9d582, 9f2d14a (#162) | v4-06-settings-appearance.png, v4-07-settings-ocean-accent.png | live Dark/Light; Scale label follows drag (200% → 130%) |
| Monitor | PASS | 620a1aa (#152) | v4-08-monitor.png, v4-08-monitor-sleep.png | search "sleep" → `sleep` pid 1338 Sleeping 0.0% 2.1 MB |
| Agents | PASS | ac106ba, 4b60522 (#161) | v4-09-agents.png | Activity row Time/Tool/Result/Detail filled, Detail is one prose row |
| Editor | PASS | 730d0c0 (#154) | v4-05-editor-rs.png, v4-05-editor-saved.png | Ctrl+S writes file, "— Edited" clears, status "saved /home/cosmos/controls.rs" |
| Terminal | PASS | 15729f2 (#155) | v4-10-terminal-scroll.png | wheel-up → "Viewing scrollback — 15 lines up" + Jump to Bottom → live prompt |
| Agents Roll Back confirm | UNTESTED | ac106ba | v4-09-agents.png | image has no snapper config, so the pane shows "snapper isn't configured" and no Roll Back button |

## D — shell

| Item | Result | SHA | Evidence | Proof |
|---|---|---|---|---|
| Dock 8px padding + centred running dot | PASS | 5e859e6 (#156) | v4-10-terminal-scroll.png (dock at left) | accent dots under running icons |
| Island agent activity + pause | PASS | 0e7cc63 (#157) | v4-12-island-agent.png | pill "test-agent · m:ss"; Pause writes `.paused`, tools/call blocks until Resume |
| Island approval pending | PASS | 0e7cc63 | v4-12b-island-approval.png | approval outranks agent in the pill |
| Island clipboard card | PASS | 0e7cc63 | v4-13-island-clipboard.png | expanded clipboard history |
| Island MPRIS now playing | UNTESTED | 0e7cc63 | — | no media player in the image; not faked |

## E — finals

| Item | Result | Proof |
|---|---|---|
| Idle RAM | PASS | 500 MiB used on the final boot (<550 MiB target) |
| Image size | PASS | 2.0G real / 3G sparse (≤3.5 GB) |
| Panics | PASS | 0 |
| Zero markers | PASS | `grep -rnE "TODO\|FIXME\|todo!\|unimplemented!\|placeholder\|mock" cosmosos/{cosmos,image,tests,provision}` = 0 outside `target/` (#158, 36b37d4) |
| Self-review top 5 | PASS | #159/#160/#161: Files name column, disabled buttons, Monitor stacked cards, Agents Time/Detail, dark control fills; all re-shot |

## Late captures (compositor IPC screenshots, main 36b37d4 @1920×1080)

| Shot | Verdict | Proof |
|---|---|---|
| v4-01-desktop-dark | PASS | Dark menubar+dock+wallpaper; Files window open showing "0 items, 7 hidden" footer |
| v4-02-desktop-light | PASS | Shell + Files window fully light after live `set_config appearance=light`; "Light" selected in CC |
| v4-10-start-glass | PASS | Start open via `toggle_launcher`: glass panel over wallpaper — search bar, PINNED apps, widgets, Log out |
| v4-11-control-centre | PASS | CC open from menubar clock: Network/Volume/Theme/Focus card + Settings/Lock/Log out row, glass over wallpaper |

All captured with the compositor IPC `{"op":"screenshot"}` op (not monitor
screendump). Note: on the first light flip, Files repainted its title bar
immediately but the body took ~3-5 s to re-render light — cosmetic lag only,
verified light on the next frame.

## Open

- Finder-style unified title bar: apps draw a 52px kit toolbar under the
  compositor's SSD title bar (two bars). Merging them needs a compositor
  chrome change.
- Agents Roll Back and MPRIS: untested, see above.
