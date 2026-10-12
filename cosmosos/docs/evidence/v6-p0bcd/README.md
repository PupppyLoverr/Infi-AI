# v6-p0bcd evidence — desk file drag + launcher-closes-menu

Boots: QEMU/KVM, `-m 2048`, real greeter login, pixman renderer, auto scale 1.25 (1920x1080),
`RUST_LOG=info,cosmos_shell=debug,smithay=debug`. Nothing merged.

## 8f7061e — set_actions DnD fix + launcher-closes-menu

- `d1c.png` — Editor drop attempt 1: Editor window opened `~/Desktop/Welcome to CosmosOS.md`
  (status bar `loaded /home/cosmos/Desktop/Welcome to CosmosOS.md`). 3/3 attempts PASS.
- `d1b-c.png` — drop-highlight ring + "Editor" label on the Editor tile mid-hover (attempt 1).
- `d3-c.png` — drop attempt 3: third Editor window open with the file.
- `d3h-c.png` — drop-highlight ring on Editor tile (attempt 3).
- `f1-c.png` — Files drop: Files window opened at `~/Desktop` (Projects + Welcome listed;
  `p.parent()` path). 2/2 PASS, Files closed beforehand.
- `t1.png` — drop on Terminal tile: refused, no window opened, icon stays at cell.
- `b1-c.png` — drop on bare desktop: refused, Welcome icon still at [0,1].
- `sw-c.png` — internal drag regression: Welcome/Projects swapped cells ([0,0]/[0,1]).
- `m6-c.png` — launcher-closes-menu check: dock menu open -> Super+Space -> menu closed,
  launcher open, "term" -> TOP HIT Terminal.
- `m2-c.png` — same check mid-flight: launcher pill open with dock menu dismissed.

Journal per accepting drop: `DEBUG cosmos_shell: dock: file drop
text=file:///home/cosmos/Desktop/Welcome%20to%20CosmosOS.md`. Zero `receive failed`,
zero `drop outside`, zero smithay DnD-cancel lines. Panics: 0.

Idle RSS ~75-95s post-login: **compositor 66124 kB, shell 26304 kB**.

## b676205 — earlier run (same branch, before set_actions)

- `d-mn.png` — bottom-dock right-click menu (Monitor): card centre x=874 vs icon centre 874
  (Delta +0.5 px); card->pill-top gap ~14-15 phys px (spec 12.5).
- `d-ah0-c.png` — auto-hide: dock strip fully hidden ~1.1s after pointer leaves
  (3 trials: last-visible 1.06-1.15s, first-hidden 1.13-1.25s).
- `d-rss6.png` — fresh-session idle RSS: **compositor 68012 kB, shell 28236 kB**
  (post-activity ~142 MB / ~69 MB retained-growth pattern).
