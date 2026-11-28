# xnulinux on an NVIDIA DGX Spark (GB10)

What was set on that machine (Ubuntu 24.04.5, Linux `7.0.0-1025-nvidia`), bare metal and in a KVM guest. The addresses
and CPU numbers are that machine's.

## Bare metal

- Secure Boot off: with it the kernel is locked down, which refuses unsigned modules and the kprobes fki needs.
- Kernel command line: `kvm-arm.mode=nvhe mem=112G nokaslr log_buf_len=128M`. `nvhe` keeps Linux at EL1; `mem=` leaves
  the RAM above it for the carve-out; with `nokaslr` the kernel image stays out of it.
- `insmod fki.ko carve_base=0x1E00000000 carve_mb=8156 cpu_list=5,6,7,8,9`. The NVMe has 15 I/O queues for 20 CPUs, so
  some CPUs share a queue's interrupt, and two XNU CPUs on such a pair deadlock: `cpu_list` gives XNU CPUs that each
  have a queue of their own (here the X925 cores; the one after the last XNU CPU is the monitor thread's).
- NetworkManager takes `xnu0` and removes its address unless told not to:
  `unmanaged-devices=interface-name:xnu0` in `/etc/NetworkManager/conf.d/`.
- The counter runs at 1 GHz: xnu.ko has XNU scale it to 24 MHz, which Apple's user space assumes.
- `xnulinux-boot.sh` keeps XNU's CPUs out of Linux's deep idle state (`LPI-1`, 133 us nominal): a hosted CPU with
  nothing to run sleeps in Linux, and a thread woken on one took 0.6 ms on average (144 ms at worst) to run, 19 us
  without it.

## GPU

The render node is NVIDIA's driver, not virtio-gpu, so `xnudrmd` serves the hosted system's Venus driver from
virglrenderer on the machine's Vulkan driver (`gpu/xnu-drm/xnudrmd_virgl.c`). Ubuntu 24.04's virglrenderer 1.0.0 fails
`vkCreateDevice` there; `gpu/build-virglrenderer.sh` builds and installs 1.2.0 with Venus. With `GPU=auto` and the
Venus driver in `/Library/GPUBundles/XnuCpuMetal.bundle`, Metal and WindowServer run on `Virtio-GPU Venus (NVIDIA GB10)`.

The backend's shared memory is Linux's 4 KB pages, not contiguous, and XnuDrm cannot map those into a task with 16 KB VM
pages (`map handle ...: 0xe00002bd`; every client then falls back to the software renderer): `xnulinux-boot.sh` starts
XNU with `vm16k=0` when xnudrmd serves this backend.

NVIDIA's driver (580) on this machine, measured natively:

- One process gets about 512 open device files and a context with a device holds 10 to 75, so the render server runs
  a process for each context (`-Drender-server-worker=process`). With every context in one process `vkCreateDevice`
  failed from about the twentieth client on and the Metal layer took the next device, the host's llvmpipe.
  Memory, with 24 contexts (a desktop after login): 4.7 GB in all as PSS, about 190 MB a process (2.7 GB anonymous,
  2.0 GB mapped files). The RSS figures add up to 9.4 GB (about 350 MB a process, 610 MB for WindowServer's) because
  each counts the driver's libraries again. One process with about 20 contexts as threads was 3.5 to 4.9 GB. The
  contexts hold 4.6 GB of GPU memory besides, either way.
- `vkCreateDevice` takes 0.33 s with one queue and 0.15 s more for each further one (16 queues: 2.6 s; destroying
  them 0.6 s). The Venus driver offers one queue of a family (`gpu/venus/patches/0007`): a Metal device is made in
  every application, with its main thread waiting.
- A host image copy (`VK_EXT_host_image_copy`) maps the whole memory object the image is in, 64 MB in 13 ms, and
  unmaps it again. The Metal layer read every frame WindowServer drew back that way: 9 to 23 frames a second with the
  GPU 2% busy. The Venus driver does not offer the feature (`gpu/venus/patches/0008`): about 50.

## In a KVM guest

`guest/`: an arm64 Ubuntu guest under KVM, QEMU run from a container (`Dockerfile`, `run-vm.sh`, `vm.sh`); KVM on the
GB10 gives the guest pointer authentication. `hcmd.sh` runs commands in the hosted system's console shell and returns
their output over the `xnu0` link, for when the kernel log is flooded.

## Open

- Without the GPU bridge `iconservicesagent` crashes in lavapipe (a JIT-compiled fragment shader reads a missing
  sampler entry); the Dock then has no icons.
- A system disk restored by a virtual Mac has its Data volume encrypted with that VM's keys: the hosted system cannot
  open it, and the volume has to be made again unencrypted.
