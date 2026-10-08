# CosmosOS budgets

Measured by `tests/measure.sh` on 2026-10-08 at c1a684d (headless KVM boot,
QEMU -m 2G -smp 2, GUEST_RES=1024x768, OVMF).

| metric | value |
|---|---|
| image file | 1.9G real / 3.0G sparse |
| boot -> cosmos-panel surface | 6.680396 s (serial marker) |
| idle RAM at desktop (used) | ~242 MiB of 1965 MiB |
| swap | zram0 1006076 kB free of 1006076 kB |

Raw evidence: work/measure/serial.log (==PAPERCUT-CHECK== block).
