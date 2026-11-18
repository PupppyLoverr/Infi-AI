# Vendored: niri

This directory is a snapshot of upstream niri — the compositor Solyra forks.

- Source: https://github.com/niri-wm/niri (branch main)
- Pinned commit: f73c14563ba3bb29e9d6dd407116ac3df0245577 (2026-11-18)
- License: GPL-3.0 (applies to everything in this directory)
- Repo origin: CosmoOS v7 plan ADR-0002 (docs/decisions.md)

## Sync

A GitHub fork (PupppyLoverr/cosmos-comp) is planned; the Amp token in this
environment cannot create repos. Once it exists, replace this snapshot with a
git subtree of the fork and push patches there:

  git remote add niri-fork https://github.com/PupppyLoverr/cosmos-comp.git
  # refresh snapshot:
  git fetch https://github.com/niri-wm/niri main && rm -rf comp && ...

Until then, upstream changes are pulled by re-cloning and replacing this
directory, keeping the pinned commit updated in this file.
