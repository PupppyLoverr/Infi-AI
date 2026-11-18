# Drive harness (kept from CosmosOS v1–v6)

QEMU QMP tooling that human-drives a guest OS: pointer moves/clicks/drags
(`qmp.sh`), screenshots (`shot.sh`), and per-feature check scripts. This is the
engine-agnostic part of the old test stack — it verified real boots with
screendumps, and Solyra reuses it from day one.

**Not yet rewired:** the scripts assume the old tree layout
(`$ROOT/cosmosos/work/image` for sockets). They get re-pointed at the new
`base/` build layout when the first image lands in P1 (PLAN.md §4).
