# #195 — logind interactive flag (Restart / Shut Down)

Image: `devin/1791553283-shell-logind-power-arg` @5d48202 + merge of main `4b61da1` (`f742c72`).

1. Cosmos menu → Restart… — **PASS**: journal `Reached target reboot.target`, `reboot: Restarting system`; guest rebooted and returned to the greeter (~90 s).
2. Cosmos menu → Shut Down… — **PASS**: `systemd-shutdown: Powering off`, `ACPI: PM: Preparing to enter system sleep state S5`, `reboot: Power down`; QEMU exited cleanly.
3. Journal — **PASS**: zero "Invalid arguments"/zbus errors from the shell.
4. Panics — **PASS**: 0.
