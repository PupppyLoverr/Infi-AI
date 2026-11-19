Solyra OS: VMware Workstation / Player (Windows, Linux)

Files
  solyra.vmx    the virtual machine (UEFI, 4 GB RAM, 4 CPUs, 3D on, SATA disk, NAT)
  solyra.vmdk   the disk (same bytes as solyra.raw; checked in CI)
  SHA256SUMS    checksums of both

Open it
  1. Unzip the solyra-vmware artifact into a folder of its own, e.g. C:\VMs\Solyra.
  2. VMware Workstation: File > Open... and pick solyra.vmx.
     (VMware Player: "Open a Virtual Machine".)
     If asked "Did you move it or copy it?", answer "I copied it".
     If asked to upgrade the virtual hardware, either answer is fine.
  3. Before the first start, check VM > Settings:
       Options > Advanced > Firmware type: UEFI, Secure Boot OFF
       Hardware > Display: "Accelerate 3D graphics" ON, graphics memory 2 GB
       Hardware > Memory: 4 GB or more; Processors: 2 or more
     The desktop needs 3D acceleration: the compositor renders with the GPU.
     Without it the session may fall back to the text login.
  4. Power on. The dev disk logs in automatically as "solyra".
     Password (sudo, the lock screen): solyra

Keys
  Super+Space  launcher            Super+Ctrl+L  lock (Solyra lock screen)
  Super+Alt+L  swaylock (recovery)  Super+Return  terminal
  Click the notch for approvals, agents, now playing.
  Settings > Appearance / Desktop / Lock Screen for every look option.
  VMware grabs the Windows key: press Ctrl+Alt to release the mouse, or set
  Edit > Preferences > Input so the VM gets Win-key combinations.

If it does not reach a desktop
  - Black screen after the boot menu: 3D acceleration is off, or the host
    GPU driver is too old for VMware's 3D. Update the host GPU driver.
  - "Operating system not found": firmware is BIOS; set it to UEFI.
  - Secure Boot error: turn Secure Boot off (the dev disk is unsigned).
  - Ctrl+Alt+F2 gives a text console (log in as solyra / solyra);
    journalctl --user -b shows the session log.
