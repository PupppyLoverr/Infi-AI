# Handoff

**Now:** Milestone 1, measuring minimal Arch + Plasma 6 Wayland (stock and
trimmed) in CI. See PLAN.md.

## Done
- PLAN.md: architecture, budgets, audit, decision gate.
- `tools/measure/` and `.github/workflows/measure.yml`: boot the Arch cloud
  image in QEMU/KVM, install `plasma-desktop`, autologin, measure 5 min after
  login, trim, reboot, measure again. Report: `out/report.txt` in artifact
  `measure-plasma`.

## Next
1. Read the measure run's report. If it fails, fix it from the logs
   (setup.log, measure.log).
2. Put the numbers in PLAN.md. If they miss the budget, show the user
   options (a) and (b) and wait.
3. Also measure the current niri + Rust panel image for option (b) (the
   existing disk.yml boots it; add the same `free -m` after login).

## How to run
- Measure locally (needs KVM): `OUT=/tmp/m tools/measure/plasma.sh`
- In CI: push to `tools/measure/**`, or dispatch the `measure` workflow.

## Open problems
- The dev container has no KVM or QEMU, Arch mirrors return 403, and the
  disk is 97% full, so the measurements only run in CI.
- llvmpipe rendering in the VM inflates kwin's RSS a little compared with
  real GPU hardware. The report states this.
