# Solyra base image

The whole OS is this container image, built by CI and booted via
[bootc](https://containers.github.io/bootc/). Updates are atomic and rollback
is native — that is what makes "undo anything an agent did" a system property
instead of a script.

## Build locally

```sh
cd base
podman build -t localhost/solyra-base:dev .
```

## Produce a bootable disk / ISO

[bootc-image-builder](https://github.com/osbuild/bootc-image-builder) converts
the container image into qcow2/raw/ISO. P1 wires this into CI with a qcow2 +
raw output and measures it (PLAN.md §7 budgets).

## Measure (every milestone)

```sh
podman image inspect localhost/solyra-base:dev --format '{{.Size}}'
```

Idle RAM and boot time are measured on the booted image in QEMU — the
`tests/drive` harness takes over once it is rewired for this layout.
