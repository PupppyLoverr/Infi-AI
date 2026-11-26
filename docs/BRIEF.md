# Brief (verbatim from the user, 2026-11-26)

> This is the source of truth. PLAN.md maps each line to work.

Build prompt: macOS Golden Gate-style Linux distribution

Role and mode
You are the lead engineer on my Linux distribution. Work autonomously: make sensible decisions, record them, and ask me only before something destructive, irreversible, costly or licence-breaking. Continue from the existing repository if there is one. Inspect it first and don't rebuild what works.

Goal
A bootable distro whose layout, behaviour, motion and glass feel as close to macOS 27 Golden Gate as Linux allows, plus my own extras. Use original or openly licensed icons, wallpapers, sounds and fonts, and our own names. Never copy Apple's assets, logos or trademarks.

Architecture (this replaces any earlier Tauri + React plan)
A. Desktop shell: use pearOS's stack
* pearOS NiceC0re is Arch Linux + a heavily customised KDE Plasma 6 (Wayland). Use the same base.
* Build the dock, top menu bar, global menu, notch, Spotlight, Control Center, notifications, widgets, launcher, lock screen and window decorations as native Plasma/KWin components (QML/Qt, C++ where needed). No Tauri or React for these.
* Glass: start from github.com/pearOS-archlinux/liquid-gel (C++ KWin effect, GPL-3.0, forked from kwin-effects-better-blur).
* Before writing a component, list the pearOS org's repos (all pages) and find what already exists. Reuse it where the licence allows. Check each repo's licence.
* pearOS's Spotlight-like search builds on KRunner. Treat its notch as unproven: it has been reported as not running smoothly.

B. Terminal app: separate, native Rust
* UI: zgx (github.com/zeronsh/zgx), a retained-mode, wgpu-based Rust GUI framework (MIT).
* Terminal and shell behaviour: port from github.com/zeronsh/zeron (MIT). It uses `alacritty_terminal` for emulation and `portable-pty` for the PTY, with PTY hosting in `crates/engine` and the terminal view in `crates/ui`.
* Zeron's UI is GPUI, not zgx. Reuse the engine-side logic, and rewrite the terminal view as a zgx component. Don't pull in GPUI.
* zgx is early (0.1, used from git, API still changing). Pin exact commits.
* Frosted glass: zgx has in-app backdrop blur, but blurring the desktop behind the window on Linux is unverified. First build a tiny spike: transparent window + KWin blur on Wayland. If zgx can't do it, add the smallest possible platform patch and document it.
* Required: tabs, splits, scrollback, selection/copy/paste, search, truecolor, Unicode, resize, themes, adjustable transparency, and the user's shell with working shell integration. Zeron's architecture doc names no shell-integration code, so search its source before claiming to reuse it.

Performance and size budgets (top priority)
Targets for the whole distro, not just the shell:
* Idle RAM: 100-150 MB, lower if possible.
* Disk: 2-3 GB installed, and an ISO under 3 GB.
* Fast boot, instant app launch and menus, animations at the display's refresh rate with no dropped frames, near-zero idle CPU.
* Beautiful and snappy. Where an effect costs speed, make it cheaper before removing it.

These idle RAM and installed-size targets are very unlikely with KDE Plasma 6. So measure before building the shell:
1. Boot a minimal Arch + Plasma Wayland VM. Record idle RAM (`free -m`, used, 5 minutes after login, VM size stated), installed size, boot time and the largest processes.
2. Trim services, indexing, unused packages, locales and firmware, then measure again. Keep hardware support working and say what was removed.
3. If it still misses, stop and show me the numbers and two options: (a) keep Plasma and pearOS components with a realistic budget, or (b) a lightweight Wayland compositor with a custom shell, with its estimated RAM and size, what pearOS work would be lost and the extra effort. Wait for my choice.

Never fake a number by excluding processes. Recheck the budgets at every milestone and treat a regression as a bug.

macOS 27 behaviour to match
* One system-wide glass transparency slider, from ultra clear to fully tinted, with readable text at every setting.
* Better diffusion of busy content behind glass, clear contrast on the active window, improved shadows.
* The same window corner radius across apps, uniform toolbars, edge-to-edge sidebars, layered app icons.
* Spotlight (Super+Space) as a "Search or Ask" bar: fast local search plus an optional AI assistant, with a pluggable provider.
* Dock indicator for apps still running in the background, with "Stop Running in Background".
* Smooth workspace and window-overview animations.
* Accessibility: Show Borders option, reduce transparency, reduce motion.
Check anything not listed here against Apple or a reputable source before building it.

My extras
(every feature https://getdroppy.app has with the notch, lock screen, etc we can do, and connects to real media.)
window arrangement and snapping options, like windows 11 windows management how we can have multiple window one screen, organised and also something that you can enable where if you open windows, it automatically arranages them.

sugest extras too. and should we continue using niri or use a better window thingo like hyperland?

If this is empty, don't invent any.

Licensing
Keep copyright notices. Keep GPL code (such as liquid-gel (if too heavy, we can req=write there liquid glass to work with ours and its changes) in GPL components, and don't copy it into the MIT-derived terminal. Maintain THIRD_PARTY.md.

Usage budget (strict)
My usage limit is shared and about 40% of the current 5-hour window was already used when I wrote this. If it hits 100%, all work stops.
* No subagents or parallel agents.
* Don't re-read files or re-research facts already given here. Search code before opening files, and read only the needed parts.
* Keep command output short (quiet flags, tail the errors).
* Build and test only what changed. No full ISO build until a milestone needs it.
* Work in small steps. After each one, commit locally and update HANDOFF.md (done, next, how to run, open problems), so a new session can resume from that file alone.
* You may not be able to see the usage meter, so never leave the repo broken. If I say usage is above 80%, finish the current edit, update HANDOFF.md and stop.
* Report in a few lines per milestone.

Milestones, in order
1. Audit the repo and pearOS components, run the RAM and disk measurements above, write PLAN.md (short) and get my choice if the targets are missed.
2. Terminal blur spike in zgx on KDE Wayland.
3. Terminal core: PTY, emulator, rendering, input.
4. Glass and theme tokens, transparency slider.
5. Menu bar and dock.
6. Spotlight, then notch, Control Center, notifications.
7. Terminal polish.
8. ISO build, VM boot test.
9. My extras.

Definition of done for each milestone
It builds, runs, has been tested on the chosen Wayland desktop, has RAM, disk and boot numbers recorded, with a screenshot if possible. State plainly what is untested, stubbed or not working. Never claim something works without running it.

---
User's note with this brief: "idk we need to meet the lightweight requirement but we need a system that can get us perfect identical macos golden gate ui/ux and our stack with rust ui/ux is not able to do that. figure out something and make the plan for this ... ask me the needed questions on the above, and consider every single character/word above in the plan include everything."
