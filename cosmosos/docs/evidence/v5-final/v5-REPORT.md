# CosmosOS v5 — report

Image: `devin/1791586600-v5-evidence` @ `d20ce7c` (main `c16af8c` + #209 + #210 + #211), fresh
account, QEMU 1920×1080 @125%, `-cpu host`. Every screenshot is a real QMP screendump of that
boot, inspected at 100% by me. Shots marked † were retaken on an image of `948ac0b` (#213, now main `318e9fc`) +
main `b932b5a`, fresh account, same settings (evidence commit `f8be871`).

## 1 — Foundations

| Item | Result | SHA / PR | Screenshot | Proof |
|---|---|---|---|---|
| 1.1 125% auto scale, crisp apps, default window size + memory | PASS | #170 #171 #174 | v5-01, v5-11 | 1920 wide → 125%; menubar 38 phys px (30 logical); new windows ≈60%×65% |
| 1.2 text contrast ≥4.5:1 | PASS | #172 #183 | v5-07, v5-12 | secondary text 4.60–8.01:1 dark, 5.60–15.08:1 light (lowest 4.60 ≥ 4.5) |
| 1.3 glass carries the wallpaper hue | PASS | #173 #175 | v5-07, v5-16 | Snap Assist empty glass samples (77–95, 25–28, 103–131) = wallpaper violet, not grey |
| 1.4 launcher dim ≤20% | PASS | #173 | v5-06 | desktop behind Start stays visible; dim alpha 0.20 |
| 1.5 translucent sidebars | PASS | #173 | v5-11, v5-19 | sidebar a(0x16,0x12,0x2A,0.78) dark; content behind shows through (see §6 diff) |

## 2 — Shell surfaces

| Item | Result | SHA / PR | Screenshot | Proof |
|---|---|---|---|---|
| 2.1 bottom-centred dock, desktop menu, clock card | PASS | #176 #177 #178 #209 | v5-01, v5-02 | dock bottom centre, 52px icons; clock card follows theme in ≈1 s |
| 2.1 weather widget | FAIL (env) | #178 | v5-01 | code is real Open-Meteo; QEMU guest had no DNS, so the card never appears |
| 2.2 Search or Ask pill + results | PASS | #179 #180 #189 | v5-03, v5-04 | "doc" → Top Hit Desktop & Dock + 3 seeded Documents |
| 2.2 Ask streams an answer | PASS | #180 #204 | v5-05 | "2+2" → streamed "4" |
| 2.3 Start above dock + widget board | PASS | #181 #182 #209 #211 | v5-06 | calendar, CPU/RAM rings, 3 real To-Dos, slideshow; board ends above footer |
| 2.4 Control Centre tiles + detail | PASS | #183 #184 #202 | v5-07, v5-07b | tiles; Wi-Fi chevron → list ("No Wi-Fi networks found": QEMU has no Wi-Fi) |
| 2.5 Dynamic Island capsule/card/clipboard | PASS | #186–#189 #203 | v5-08†, v5-09†, v5-10 | real Cosmos Helper activity + pending approval; text + image clips |
| 2.6 context menus + Ask Cosmos | PASS | #190 #191 #205 | v5-13, v5-14† | Files right-click → Ask Cosmos › Summarize → result sheet |
| 2.7 snap flyout + Snap Assist | PASS | #192 #193 | v5-15, v5-16, v5-16b | glass flyout, 5 groups; Snap Assist live thumbnails |
| 2.8 lock screen + wrong password | PASS | #194 #195 | v5-21, v5-22, v5-22b | glass card, 96pt clock; shake frame offset ≈14px; "Incorrect password" |
| 2.9 app density + Files preview/Quick Look | PASS | #196 #210 | v5-11, v5-12 | sidebar 272–280 phys (=220 @125%), rows 36 phys (=28) |

## 3 — Apps

| Item | Result | SHA / PR | Screenshot | Proof |
|---|---|---|---|---|
| 3a Monitor charts | PASS | #197 | v5-18 | 4 cards, fading fills, 60 s history, real Net/Disk counters |
| 3b Editor header | PASS | #198 | — (boot-tested in #198) | filename only; unsaved dot appears/clears on Ctrl+S |
| 3c Terminal 92% dark glass | PASS | #199 #206 | v5-11 | body shows content behind; idle Terminal ≈0% CPU |
| 3d Cosmos Helper + Agents + rollback | PASS | #200 #201 | v5-19, v5-20 | live activity; Restore brought back a deleted file in ≈5 s |

## 4 — Seed content

| Item | Result | SHA / PR | Screenshot | Proof |
|---|---|---|---|---|
| 4 guide .md + PDF, budget CSV, 4 photos, hello-cosmos | PASS | #201 #210 | v5-04, v5-12, v5-06 | Documents searchable; Pictures grid shows 4 real thumbnails; slideshow uses them |

## 5 — Measurements

- **Idle RAM** (fresh account, 60 s idle, no apps): `free -m` used 770 MiB of 7939.
- **Top RSS (KiB):** cosmos-compositor 323720, cosmos-shell 95608, Xwayland 86908, NetworkManager 20132, systemd 15348 (full top-15 in `index.md`).
- **Image size:** `dist/cosmosos-x86_64.raw` 3,221,225,472 bytes sparse, 2.1 G allocated.
- **Panics:** 0. (`journalctl -b | grep -ic panic` printed 1: the grep command line itself, echoed into the journal.)
- **Idle compositor CPU:** ≈4.7% (was 150–190% before #207).
- **Prohibited terms** (`TODO|FIXME|todo!|unimplemented!|placeholder|mock`) in `cosmosos/`: 0 matches after #212.
- **Glass hue:** dark window a(0x1C,0x17,0x30,0.90), sidebar a(0x16,0x12,0x2A,0.78); light window a(0xFB,0xFA,0xFE,0.92), sidebar a(0xEC,0xE8,0xF5,0.90). Glass samples the wallpaper at its screen position.

## 6 — Reference comparisons and self-review

Side-by-sides: `cmp-*.png`. Three honest differences each; the top one per round was fixed and reshot.

| Ref | Shot | Differences |
|---|---|---|
| ref-1 owner sketch | v5-01 | (1) sketch's dock is a left rail; ours is bottom-centred, as you chose later. (2) sketch wants a Dropover-style shelf from the notch; ours is the island's Shelf tab, invisible until opened. (3) v5-01's menubar had no date text: it first paints ≈40 s after sign-in, so the shot was early. Reshot† shows "Sat 10 Oct 00:40" (next fix: draw the clock on the first frame; the menubar also reads 00:40 while the desktop card reads 00:41) |
| ref-2 Win11 snap layouts | v5-15 | (1) Win11's flyout is small and sits under the maximise button; ours is larger and covers the Files toolbar. (2) Win11 zones are light with clear gaps; ours are low-contrast blue-grey. (3) no hovered zone in the shot |
| ref-3 macOS Search | v5-04 | (1) ref pill is short with a mic icon; ours is 680px with "Tab to Ask", no mic. (2) ref glass is neutral; ours takes the violet wallpaper. (3) our file rows use one grey glyph, not file-type icons |
| ref-4 macOS menu | v5-13 | (1) macOS opens the submenu to the right; ours drops below and covers "Move to Trash". (2) macOS rows ≈22px; ours are taller. (3) Summarize/Explain have no icons, Open in Agent does |
| ref-5 macOS Mail | v5-11 | **no image in the repo** (`NOTES.md`: "awaiting image"). Compared to its numbers only: sidebar 220, rows 28, 13/11pt: all met |
| ref-6 purple concept | v5-09 | (1) island used letter avatars ("C", "E") → **fixed** (sparkle glyph, `5f02289`), reshot†. (2) approval toast body showed raw "{}" → **fixed** (`948ac0b`), reshot†. (3) approval line truncates and the card has empty space below |
| ref-7 Win11 widgets | v5-06 | (1) no weather (guest has no DNS). (2) Win11 cards are bigger, light acrylic; ours are dark and compact. (3) dock "Launcher" tooltip is clipped under Start |
| ref-7b Win11 Start | v5-06 | (1) Win11 groups apps into category folders; ours is pinned + recommended + widgets. (2) light vs dark glass. (3) search hint "Search or Ask — > command, ? question" reads technical |

Also fixed in round 1: the Ask result sheet showed opencode's "> build · big-pickle" banner (`5f02289`, v5-14†).

### Round-1 reshoot (checked at 100%)
- v5-08†, v5-09†: both agent rows (Cosmos Helper, default) and the capsule show the sparkle glyph. No letters. The toast and the island row read "default wants a screenshot of the desktop", with no "{}".
- v5-14†: Summarize gave a real ≈60-word summary of the Welcome guide, with no "> build" line.
- v5-01†: menubar date and clock present. 0 panics on the reshoot boot.

### Not fixed (known)
- Menubar clock: first paint ≈40 s after sign-in, and up to a minute behind the desktop clock card.
- Ask Cosmos submenu placement (ref-4 #1), snap-zone contrast (ref-2 #2), weather without network, Files sidebar showing Terminal text behind it (v5-11), Files grid names wrapping mid-word (v5-12), clipboard text wrapping mid-word (v5-10), a second glass edge peeking out of Control Centre (v5-07).
- Login screen black blocks: a QEMU virtio-gpu display artefact (in-session capture is clean), not CosmosOS.
- Not tested: Wi-Fi join, audio output, Now Playing (QEMU has no Wi-Fi/audio/media player).
