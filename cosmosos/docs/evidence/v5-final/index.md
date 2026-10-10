# v5 §5 evidence set

Fresh image from `devin/1791586600-v5-evidence` @ `d20ce7c` (origin/main `c16af8c` + #209 clock-theme + #210 files-grid-thumbs + #211 start-volume-drag; #212 lock-comment landed on main `b932b5a` during the run — comment-only, not in this image).
Fresh account, 1920×1080 @125%, default settings, default bottom-centred dock, default `run.sh` (`-cpu host`). Every shot is a real QMP screendump.

| Shot | What's in it | Honest gap |
|------|--------------|------------|
| v5-01.png | Desktop, dark, violet wallpaper: menubar, island idle, dock, weather + clock cards | — |
| v5-02.png | Desktop, light, ocean wallpaper | Weather card is absent on the light desktop (same as dark set — card region shows clock only) |
| v5-03.png | Search/Ask idle pill (Super+Space) | — |
| v5-04.png | Search results for "doc" | — |
| v5-05.png | Ask mode, edge glow, streamed answer ("4" for 2+2) | — |
| v5-06.png | Start widgets: 3 todo items added via UI, calendar, CPU/RAM rings, volume, Photos card | — |
| v5-07.png | Control Centre tiles | — |
| v5-07b.png | CC Wi-Fi detail (empty list — QEMU has no Wi-Fi) | Empty network list is expected in QEMU |
| v5-08.png | Island compact with Cosmos Helper activity timer | — |
| v5-09.png | Island expanded, Activities + real pending approval (agent file write into ~/Documents) | — |
| v5-10.png | Island Clipboard: one text clip + one image clip (real wl-copy on wayland-1) | — |
| v5-11.png | Files dark, list view, preview pane on a file | — |
| v5-12.png | Files light, grid view ~/Pictures — 4 real photo thumbnails (#210) | Filenames wrap ("…g"/"png" split) — cosmetic, reported |
| v5-13.png | Right-click a file → Ask Cosmos submenu open | — |
| v5-14.png | Summarize result sheet | Sheet streams via Cosmos Helper but resolves to "Couldn't answer — set up a model provider" — no LLM provider on a fresh account. Sheet + provider-wall state captured honestly |
| v5-15.png | Snap flyout on the green button (zone groups rendered; Files maximized behind) | — |
| v5-16.png | Files snapped left + Snap Assist card on free right half with live Editor tile | — |
| v5-16b.png | Settled state: Files left half, Editor right half | Extra shot (not in spec) |
| v5-17.png | Settings → Appearance: scale, wallpaper, accent | — |
| v5-18.png | Monitor: 4 chart cards (CPU/Memory/Network/Disk) + process table | Network/Disk flat at 0 B/s — no traffic in QEMU |
| v5-19.png | Agents: "evidence" agent Activity tab — real audited calls (Screenshot "needs approval", 2× Write file "Denied — outside write roots") | Cosmos Helper's own Activity tab is empty — the earlier Summarize didn't register an audited call there |
| v5-20.png | Agents → Rollback: Take Snapshot, Home #1 "First boot" + Restore…, System #3/#2/#1 + Roll Back… | — |
| v5-21.png | Lock screen: clock, date, user card, password field | — |
| v5-22.png | Wrong-password shake frame: card displaced ~14 px with "Incorrect password" visible | Shake is <300 ms; caught at ~2.7 s post-Enter on the fourth attempt (QMP screendump cadence ~450 ms) |
| v5-22b.png | Settled lock state after shake: "Incorrect password", field cleared | — |

## Metrics (60 s idle, desktop, no apps open)

```
free -m:
               total        used        free      shared  buff/cache   available
Mem:            7939         770        6585         127         938        7168
Swap:           3969           0        3969

top-15 RSS (KiB):
 745  cosmos-compositor  323720
 759  cosmos-shell        95608
 766  Xwayland            86908
 566  NetworkManager      20132
   1  systemd             15348
 346  systemd-resolved    14748
 673  wireplumber         13912
 303  systemd-journal     12872
 657  systemd (user)      12552
 364  systemd-networkd    11520
 559  upowerd             10344
 669  pipewire            10336
 352  systemd-udevd        9952
 554  systemd-logind       9440
 590  polkitd              8508

Image: dist/cosmosos-x86_64.raw — 3,221,225,472 B (3.0 GiB sparse), 2.1 G allocated
Panics (journalctl -b | grep -ic panic): 0 real
  (naive count is 1 — it matches the grep command's own text echoed in the journal via sudo)
Idle compositor, second top -b -d 4 -n 2 sample, no apps: 4.7% CPU
```

## #210 / #211 boot-verification note

Both were verified on this same live image (`d20ce7c`) **before** merging:
- #210 files-grid-thumbs: 4 real photo thumbnails in ~/Pictures grid, list view unchanged, folder re-entry re-rendered thumbs with no flicker, zero `thumbnail decode failed`, Files ~0% CPU in grid, 0 panics.
- #211 start-volume-drag: press on the volume track + slow sweep moved the fill live both directions, release outside the card ended the drag, other Start widgets unaffected. Mute-glyph toggle ambiguous in QEMU (no sink).
