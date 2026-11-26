# Handoff

**Now:** PLAN.md rewritten around option (c) (Hyprland + Quickshell + Rust
services + zgx terminal). Waiting for the user's answers (PLAN.md §8). Brief
verbatim in docs/BRIEF.md.

## Done
- PLAN.md: architecture, budgets, audit, decision gate.
- `tools/measure/` and `.github/workflows/measure.yml`: boot the Arch cloud
  image in QEMU/KVM, install `plasma-desktop`, autologin, measure 5 min after
  login, trim, reboot, measure again. Report: `out/report.txt` in artifact
  `measure-plasma`.

## Next
1. Wait for the user's choice. Build nothing shell-side before it.
2. Also measure the current niri + Rust panel image for option (b) (the
   existing disk.yml boots it; add the same `free -m` after login).

## How to run
- Measure locally (needs KVM): `OUT=/tmp/m tools/measure/plasma.sh`
- In CI: push to `tools/measure/**`, or dispatch the `measure` workflow.

## Open problems
- The dev container has no KVM or QEMU, Arch mirrors return 403, and the
  disk is 97% full, so the measurements only run in CI.
- llvmpipe rendering in the VM inflates kwin's RSS a little compared with
  real GPU hardware. The report states this.
