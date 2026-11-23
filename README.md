# Solyra OS

A Linux distro in development for humans and AI agents. The goals are a fast,
customizable desktop, an installed footprint of at most 4 GB, and isolated agent
workspaces with explicit permissions and rollback for supported actions.

> **Working name.** "Solyra" is the project name during P0–P2; a final branding-time
> name review happens before any public release (see PLAN.md §8).

## What it is

- **Base:** Fedora bootc — the OS is built as a container image. Atomic updates and
  rollback are goals for the release pipeline, not features of the development disk.
- **Window management:** [niri](https://github.com/niri-wm/niri) provides scrollable
  tiling. Master/stack and Snap Assist are planned.
- **Shell and apps:** planned around [zgx](https://github.com/zeronsh/zgx), a
  retained-mode Rust GUI framework. The current image uses upstream Foot,
  Thunar, Fuzzel, and swaylock; zui (the GPUI fork) is another option for
  first-party apps.
- **Agents:** the plan uses [zeron](https://github.com/zeronsh/zeron) for isolated
  desktop spaces, approval prompts, an audit trail, and snapshot-based undo for
  supported changes.
- **Setup:** a first-boot setup wizard in the Windows OOBE / macOS Setup Assistant
  tradition — network, account, appearance, and optional layers like Gaming.

## Status

Phase 2 (window management) is in progress. A cold-booted x86_64 disk reaches
a working niri session in QEMU: tiling, focus, the workspace overview, and the
terminal/file-manager shortcuts all respond, with the whole desktop using about
341 MiB at idle. Snap zones and a hardware run remain. See
[the build instructions](base/README.md), [measured budgets](docs/budgets.md),
and [the roadmap](PLAN.md).

The previous implementation (CosmosOS v1–v6) is preserved on the `cosmos-legacy` branch.

A development disk is built by the `disk` GitHub Actions workflow; see
[base/README.md](base/README.md#get-a-ready-made-disk-ci) for downloading and
running it in a VM.

The desktop takes the best of macOS and Windows (see
[docs/design-research.md](docs/design-research.md)): a Finder-style menu bar,
a notch or Island that carries agent approvals and now playing, four
dock variants (including a Windows-style taskbar), Solid / See-through /
Glass / Acrylic materials per surface, and macOS- and Windows-style lock
screens. Every option is in Settings. To try it in VMware on Windows, use the
`solyra-vmware` artifact (see [base/vmware/README.txt](base/vmware/README.txt)).

What exists beyond the shell: zgx apps (Settings, Terminal with tabs, Browse),
the `solyra-agent` runtime pieces (approval hooks for Claude Code, Codex and an opencode bridge, a terminal wrapper the notch
can type into, a bubblewrap sandbox with undo), notifications, HiDPI, and a
CI-built test disk. What does not: a first-boot login flow, an installer ISO,
screen-reader support in the shell, fractional scaling, snap zones (niri is a
scrolling tiler; layout presets and floating toggles are provided instead).

## Layout

```
PLAN.md            — the living build plan
docs/              — design language, agent-platform design, budgets
assets/wallpapers/ — original wallpaper art
tests/drive/       — QEMU QMP drive harness (human-drives-the-VM test tooling)
base/  comp/  shell/  apps/  agent/   — land in P1+ (see PLAN.md §3)
```
