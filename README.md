# Solyra OS

A Linux distro built for humans **and** AI agents on the same desktop — beautiful,
tiny (≤ 4 GB), fast, and forgiving: nothing an agent does is unrecoverable.

> **Working name.** "Solyra" is the project name during P0–P2; a final branding-time
> name review happens before any public release (see PLAN.md §8).

## What it is

- **Base:** Fedora Atomic (bootc) — the whole OS is a container image; updates are
  atomic, rollback is native, images build in CI.
- **Window management:** a fork of [niri](https://github.com/YaLTeu/niri) — scrollable
  tiling by default, plus master/stack, floating, and snap layouts with Snap Assist.
- **Shell and apps:** written in [zgx](https://github.com/zeronsh/zgx) — a retained-mode
  Rust GUI framework (wgpu rendering, fine-grained updates, idle windows do zero work).
  zui (our GPUI fork) is used where immediate mode genuinely wins.
- **Agents:** [zeron](https://github.com/zeronsh/zeron) as the agent control plane.
  Agents get their own desktop space, a policy sandbox, approval prompts in the UI,
  a full audit trail, and one-click rollback of anything they did.
- **Setup:** a first-boot setup wizard in the Windows OOBE / macOS Setup Assistant
  tradition — network, account, appearance, and optional layers like Gaming.

## Status

Phase 0 (repo reset) — see [PLAN.md](PLAN.md) for the full plan, phases, and budgets.
The previous implementation (CosmosOS v1–v6) is preserved on the `cosmos-legacy` branch.

## Layout

```
PLAN.md            — the living build plan
docs/              — design language, agent-platform design, budgets
assets/wallpapers/ — original wallpaper art
tests/drive/       — QEMU QMP drive harness (human-drives-the-VM test tooling)
base/  comp/  shell/  apps/  agent/   — land in P1+ (see PLAN.md §3)
```
