#!/bin/bash
# SPDX-License-Identifier: MIT
# Build and run fence.c, on Linux (the kernel's DRM headers) or on a Mac with DRM=<a Mesa tree>/include/drm-uapi.
set -euo pipefail
DRM=$(cd "${DRM:-/usr/include/drm}" && pwd)	# absolute: it may be relative to the caller's directory, and is linked to from $T
cd "$(dirname "$0")"
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
ln -s "$DRM" "$T/drm-uapi"
# (Linux's __u64 is unsigned long long, not uint64_t)
${CC:-cc} -g -O1 -D_GNU_SOURCE -Wall -Wno-incompatible-pointer-types ${CFLAGS:-} -I. -I"$T" -I.. -I../darwin fence.c -o "$T/fence" -lpthread
"$T/fence"
