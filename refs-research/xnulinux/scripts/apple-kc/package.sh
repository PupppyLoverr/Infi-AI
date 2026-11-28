#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# In the VM, after scripts/vm-xnu-rebuild.sh: the kit a user links Apple's kexts with on their own machine
# (scripts/apple-kc/link.sh), so that what is distributed holds nothing of Apple's:
#   KIT/kernel.macho     the hosted kernel, as vm-xnu-rebuild.sh linked it (its segment slots fit macOS 27.2's collection)
#   KIT/kexts/NAME/      xnulinux's own kexts, the objects and the description scripts/kclink links them from
#   KIT/kc-free.bin      the collection without Apple's kexts (scripts/free-kc/build.sh), when it was built
#   KIT/kit.json         what link.sh needs to make the same collection vm-xnu-rebuild.sh makes: the kernelcache it must
#                        be given (sha256) and where Apple has it, the Apple kexts to keep (scripts/apple-kc/keep.txt),
#                        the pointers to rebind (scripts/apple-kc/rebind.txt), the kexts of ours, AMFI's platform version
#   scripts/apple-kc/package.sh [KIT]          (default ~/w/apple-kc-kit)
set -euo pipefail
POC=$(cd "$(dirname "$0")/../.." && pwd)
DVM=${DVM:-$HOME/w/dvm}; KIT=${1:-$HOME/w/apple-kc-kit}
[ -s "$DVM/out/k27/natives.txt" ] || { echo "$DVM/out/k27/natives.txt: run scripts/vm-xnu-rebuild.sh (with out/kc/kc27.bin) first" >&2; exit 1; }
[ -s "$DVM/out/kc/kc27.bin" ] || { echo "$DVM/out/kc/kc27.bin: the kernelcache the kit is for is not there (kit.json names its sha256)" >&2; exit 1; }
mapfile -t lines < "$DVM/out/k27/natives.txt"	# one kext description a line, paths whole (a file not read stops here)
natives=(); for l in "${lines[@]}"; do [ -n "$l" ] && natives+=("$l"); done
# KIT is replaced whole: only a kit (or nothing) may be there, not a directory that holds something else
[ ! -e "$KIT" ] || [ -e "$KIT/kit.json" ] || { [ -d "$KIT" ] && [ -z "$(ls -A "$KIT")" ]; } ||
	{ echo "$KIT: exists and is not a kit (no kit.json): not replacing it" >&2; exit 1; }
rm -rf "$KIT"; mkdir -p "$KIT/kexts"
cp "$DVM/out/k27/kernel-k27.macho" "$KIT/kernel.macho"
[ -e /lib/firmware/xnu/kc-free.bin ] && cp /lib/firmware/xnu/kc-free.bin "$KIT/kc-free.bin"
python3 - "$DVM" "$KIT" "$(sha256sum "$DVM/out/kc/kc27.bin" | cut -d' ' -f1)" \
	"$(git -C "$POC" rev-parse HEAD 2>/dev/null || echo unknown)" "${AMFI_PLATFORM:-26}" "${natives[@]}" -- \
	$(grep -v '^#' "$POC/scripts/apple-kc/keep.txt") -- $(grep -v '^#' "$POC/scripts/apple-kc/rebind.txt") <<'PY'
import json, os, shutil, sys
from pathlib import Path
dvm, kit, kc_sha, poc_rev, amfi = sys.argv[1:6]
rest = sys.argv[6:]
natives, rest = rest[:rest.index('--')], rest[rest.index('--') + 1:]
keep, rebind = rest[:rest.index('--')], rest[rest.index('--') + 1:]
kit = Path(kit)
specs = []
for path in natives:
    spec = json.loads(Path(path).read_text())
    name = spec['name']
    objects = Path(dvm, spec['objects'])     # relative to macOS-DroidVM's root, where vm-xnu-rebuild.sh links
    d = kit / 'kexts' / name
    d.mkdir(parents=True)
    copied = []
    for o in json.loads(objects.read_text()):
        assert not (d / Path(o).name).exists(), (name, o)
        shutil.copy(o, d / Path(o).name)
        copied.append(f'kexts/{name}/{Path(o).name}')
    (d / 'objects.json').write_text(json.dumps(copied, indent=1) + '\n')
    spec['objects'] = f'kexts/{name}/objects.json'   # relative to the kit, where link.sh runs kclink
    (d / f'{name}.json').write_text(json.dumps(spec, indent=2) + '\n')
    specs.append(f'kexts/{name}/{name}.json')
seen = set()
keep = [k for k in keep if k and not (k in seen or seen.add(k))]
(kit / 'kit.json').write_text(json.dumps({
    'kernelcache_sha256': kc_sha,
    'kernelcache': "macOS 27.2 (26B123) kernelcache.release.vma2, unpacked from its IM4P",
    'ipsw': json.loads(Path(dvm, 'configs/macos-27.2-26B123.json').read_text())['ipsw']['url'],
    'xnulinux_rev': poc_rev,
    'amfi_platform': int(amfi),
    'keep': keep,
    'rebind': rebind,
    'native': specs,
}, indent=1) + '\n')
print(f'{kit}: kernel, {len(specs)} kexts of ours, {len(keep)} Apple kexts to keep')
PY
du -sh "$KIT"
