# CosmosOS budgets

Measured by `tests/measure.sh` on 2026-10-08 at c7451e1 (headless KVM boot,
QEMU -m 2G -smp 2, GUEST_RES=1024x768, OVMF).

| metric | value |
|---|---|
| image file | 2.0G real / 3.0G sparse |
| boot -> cosmos-panel surface | 11.763154 s (serial marker) |
| idle RAM at desktop (used) | ~266 MiB of 1965 MiB |
| swap | zram0 1006076 kB free of 1006076 kB |

Raw evidence: work/measure/serial.log (==PAPERCUT-CHECK== block).

Visual-blitz boot @5489618 (1920x1080, files+editor+settings+monitor open):
MemTotal 2014560 kB, MemAvailable 1454524 kB (~546 MiB unavailable
incl. apps; ~340 MiB headless-idle delta vs the 1024x768 measure).

v3 perf round @2498db6 (2026-10-08): idle RAM 484 MiB @1920x1080;
30-min soak (files+editor+terminal) free -m used 573->622 MiB (+8.5%),
compositor RSS +1.6%, zero panics; terminal RSS +39.9% (FAILS <10%: one step t0->t1, flat after) — docs/evidence/v3-soak.txt;
image 2.0G real / 3.0G sparse.

uitk shm buffer reuse (#147): terminal RSS bounded — +56.7% t0->t5 pool
warmup, then flat t5->t30; strict <10% rule still FAILS for terminal
(warmup counted from t0); compositor +4.2%, shell +0.3%, files +7.0%,
editor flat, zero panics — docs/evidence/v3-soak-after.txt
