#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# Where the kernel is built (after scripts/vm-xnu-rebuild.sh): xnulinux's corecrypto headers (xnu-patches/corecrypto)
# against the ones XNU ships, for the interface's ABI. Every kernel source that includes <corecrypto/...> is compiled
# twice with its own recipe (the k27 object tree's .o.json for C, .cpo.json for C++), once with each set of headers
# first in the include path;
# the objects' code, data, relocations and section sizes (debug information aside) must be the same. XNU's headers are read from the k27
# tree's git history into a temporary directory, never kept.
#   scripts/corecrypto-abi-check.sh            (DVM: ~/w/dvm)
set -euo pipefail
POC=$(cd "$(dirname "$0")/.." && pwd)
DVM=${DVM:-$HOME/w/dvm}; T=$DVM/out/k27/xnu
OBJ=$DVM/out/x12377/build/xnu/k27-obj/DEVELOPMENT_ARM64_BCM2837
export PATH=$PATH:/usr/lib/llvm-21/bin
W=$(mktemp -d); trap 'rm -rf $W' EXIT
[ -d $OBJ ] || { echo "no k27 objects at $OBJ: build the kernel first (scripts/vm-xnu-rebuild.sh)"; exit 1; }
mkdir -p $W/old $W/new/corecrypto
git -C $T archive HEAD EXTERNAL_HEADERS/corecrypto | tar -x -C $W/old --strip-components=1
cp $POC/xnu-patches/corecrypto/include/corecrypto/*.h $W/new/corecrypto/
# the sources that include the interface, xnulinux's kernel side (osfmk/xnulinux_crypto) and the PRNG's registration
# (osfmk/prng) among them
SRCS=$(cd $T && grep -rlE '#include <corecrypto/' --include=*.c --include=*.cpp bsd osfmk libkern iokit security san pexpert |
	grep -vE '^osfmk/(arm64/)?corecrypto/' | sort)
python3 - $W $OBJ $SRCS <<'PY'
import json, os, subprocess, sys
from pathlib import Path
w, obj, srcs = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3:]
recipes = {}
for j in [*obj.rglob('*.o.json'), *obj.rglob('*.cpo.json')]:
    r = json.JSONDecoder().raw_decode(j.read_text())[0]
    recipes.setdefault(os.path.normpath(os.path.join(r['directory'], r['file'])), r)
# the one source the k27 configuration does not build (NFS's Kerberos); every other one must be compared
UNBUILT = {'bsd/nfs/gss/ccrypto.c'}
same = differ = missing = 0
for s in srcs:
    r = next((v for k, v in recipes.items() if k.endswith('/' + s)), None)
    if r is None:
        if s in UNBUILT:
            print(f'  -   {s} (not built in this configuration)')
        else:
            missing += 1; print(f'  MISSING {s}: no recipe in {obj}')
        continue
    outs = {}
    for side in ('old', 'new'):
        args, cmd, i = r['arguments'], [], 0
        while i < len(args):
            a = args[i]
            if a == '-o': cmd += ['-o', str(w / f'{side}.o')]; i += 2; continue
            if a in ('-MF', '-MJ', '-MT', '-MQ'): i += 2; continue
            if a in ('-MD', '-MMD', '-MP'): i += 1; continue
            cmd.append(a); i += 1
        cmd.insert(1, f'-I{w / side}')
        p = subprocess.run(cmd, cwd=r['directory'], capture_output=True, text=True)
        if p.returncode:
            print(f'  ERR {s} ({side} headers):\n' + '\n'.join(p.stderr.splitlines()[:15])); sys.exit(1)
        # code, data and relocations of every section but the debug ones (__DWARF: their sizes move the zero-fill
        # sections' addresses, which are left out), and the sections' sizes
        o = str(w / f'{side}.s.o')
        subprocess.run(['llvm-objcopy', '--strip-debug', str(w / f'{side}.o'), o], check=True)
        dump = subprocess.run(['llvm-objdump', '-d', '-r', '-s', '--no-show-raw-insn', o], capture_output=True, text=True,
                              check=True).stdout.splitlines()[2:]
        dump = [l for l in dump if not l.startswith('<skipping contents of bss section')]
        heads = subprocess.run(['llvm-objdump', '-h', o], capture_output=True, text=True, check=True).stdout.splitlines()
        heads = [' '.join(l.split()[1:3]) for l in heads if l.strip()[:1].isdigit() and '__DWARF' not in l and 'debug' not in l]
        outs[side] = '\n'.join(heads + dump)
    if outs['old'] == outs['new']:
        same += 1; print(f'  ok  {s}')
    else:
        differ += 1; print(f'  DIFF {s}')
        for side in ('old', 'new'):
            (w.parent / f'abi-{Path(s).name}.{side}.txt').write_text(outs[side])
print(f'{same} the same, {differ} different' + (f', {missing} without a recipe' if missing else ''))
sys.exit(1 if differ or missing or same == 0 else 0)
PY
