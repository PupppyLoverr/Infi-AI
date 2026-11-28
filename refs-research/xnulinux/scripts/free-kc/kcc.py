#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Compile a kext's sources as relocatable arm64e objects with the hosted kernel's own compile flags (its compile records
in the k27 object tree), as macOS-DroidVM's build-native-kext.py does for xnulinux's own kexts, but for third-party kext
sources: no -Werror, per-kext includes, defines and flags, and two views of the kernel headers:

  default      the kernel's own (its include paths and XNU_KERNEL_PRIVATE), for sources written against xnu's tree
  --kext-view  what a kext sees: this kernel's Kernel.framework (Headers, PrivateHeaders; scripts/free-kc/build.sh makes
               it with xnu's installhdrs) instead of the kernel's include paths and kernel-internal macros

--macos builds for macOS (TARGET_OS_OSX) instead of the kernel's iOS target: the free kexts' sources pick their macOS
configuration from it (HFS, IOStorageFamily's virtual methods); the code is the same arm64e kernel ABI.

    kcc.py OUTDIR [--kext-view] [--macos] [-I DIR] [-D X] [--after DIR] [--cflag F] [--drop-def NAME] SOURCE...

Objects (named after their path below the current directory) and OUTDIR/objects.json, the list scripts/kclink/kclink.py
--native reads.
"""
import argparse, json, os, subprocess, sys
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
DVM = Path(os.environ.get('DVM', Path.home() / 'w/dvm'))
NETINC = Path(os.environ.get('NETINC', Path(__file__).resolve().parents[2] / 'build/netinc'))   # TargetConditionals.h (vm-xnu-rebuild.sh)
KF = Path(os.environ.get('KERNEL_FRAMEWORK', DVM / 'out/x12377/build/xnu/k27h-dst/System/Library/Frameworks/Kernel.framework/Versions/A'))
tree = DVM / 'out/x12377/build/xnu/k27-obj/DEVELOPMENT_ARM64_BCM2837'
p = argparse.ArgumentParser()
p.add_argument('out'); p.add_argument('sources', nargs='+')
p.add_argument('-I', dest='inc', action='append', default=[]); p.add_argument('-D', dest='defs', action='append', default=[])
p.add_argument('--cflag', action='append', default=[]); p.add_argument('--macos', action='store_true', help='build for macOS (TARGET_OS_OSX), as the kexts expect; the kernel is the same arm64e ABI')
p.add_argument('--after', action='append', default=[], help='-idirafter DIR'); p.add_argument('--cxxflag', action='append', default=[])
p.add_argument('--c-recipe', default='bsd/DEVELOPMENT/bsd_init.o.json')
p.add_argument('--drop-def', action='append', default=[], help='a -D of the kernel recipe to leave out')
p.add_argument('--kext-view', action='store_true', help='drop the kernel-internal macros (XNU/MACH/BSD_KERNEL_PRIVATE...): headers as a kext sees them')
a = p.parse_args()
INTERNAL = {'XNU_KERNEL_PRIVATE', 'MACH_KERNEL_PRIVATE', 'BSD_KERNEL_PRIVATE', 'IOKIT_KERNEL_PRIVATE', 'LIBKERN_KERNEL_PRIVATE', 'XNU_KERN_EVENT_DATA_IS_VLA'}
rec = {k: json.JSONDecoder().raw_decode((tree / n).read_text())[0] for k, n in (('cpp', 'iokit/DEVELOPMENT/IOPlatformExpert.cpo.json'), ('c', a.c_recipe))}
out = Path(a.out).resolve(); out.mkdir(parents=True, exist_ok=True)
def one(src):
    src = Path(src).resolve(); kind = 'cpp' if src.suffix in ('.cpp', '.cc') else 'c'
    r = rec[kind]
    try:
        rel = src.relative_to(Path.cwd())
    except ValueError:
        rel = Path(src.parent.name) / src.name
    dst = out / (str(rel.with_suffix('')).replace('/', '_') + '.o'); cmd = []; args = r['arguments']; i = 0
    while i < len(args):
        v = args[i]
        if v == r['file']: cmd.append(str(src))
        elif v == '-o': cmd += ['-o', str(dst)]; i += 1
        elif v in ('-MF', '-MJ', '-MT', '-MQ'): i += 1
        elif v == '-Xclang' and args[i + 1].startswith('-W'): i += 1
        elif a.kext_view and v in ('-I', '-isystem', '-iquote', '-include', '-idirafter'): i += 1
        elif a.kext_view and (v.startswith(('-I', '-isystem', '-iquote', '-idirafter')) or (v.startswith('-include') and len(v) > 8)): pass
        elif v == '-D' and args[i + 1].split('=')[0] in a.drop_def: i += 1
        elif v.startswith('-D') and v[2:].split('=')[0] in a.drop_def: pass
        elif a.kext_view and v == '-D' and args[i + 1].split('=')[0] in INTERNAL: i += 1
        elif a.kext_view and v.startswith('-D') and v[2:].split('=')[0] in INTERNAL: pass
        elif a.macos and v.startswith('--target=') and '-ios' in v: cmd.append(v.replace('-ios', '-macos'))
        elif a.macos and v.startswith('-mios-version-min='): cmd.append('-mmacos-version-min=' + v.split('=')[1])
        elif v not in ('-MD', '-MMD', '-MP', '-g', '-Werror') and not v.startswith('-W'): cmd.append(v)
        i += 1
    # the kext's own headers first (they may shadow the kernel's copies of the same family headers)
    extra = [f'-I{d}' for d in a.inc] + [f'-D{d}' for d in a.defs] + a.cflag + (a.cxxflag if kind == 'cpp' else []) + ['-w'] + [x for d in a.after for x in ('-idirafter', d)]
    if a.kext_view:   # the kernel's own include paths are gone: the headers a kext builds against (Kernel.framework of this kernel)
        extra += [f'-I{KF}/PrivateHeaders', f'-I{KF}/Headers', '-idirafter', str(DVM / 'out/k27/xnu/EXTERNAL_HEADERS'), '-idirafter', str(NETINC), '-D__startup_func=', '-DDRIVER_PRIVATE', '-DNeXT']
    cmd[1:1] = extra
    env = dict(os.environ, CCC_OVERRIDE_OPTIONS='+-Wno-error')
    res = subprocess.run(cmd, cwd=r['directory'], capture_output=True, text=True, env=env)
    return src, dst, res
objs, failed = [], 0
with ThreadPoolExecutor(6) as ex:
    for src, dst, res in ex.map(one, a.sources):
        if res.returncode:
            failed += 1; print(f'FAILED {src}\n' + '\n'.join((res.stdout + res.stderr).splitlines()[:25]))
        else: objs.append(str(dst))
(out / 'objects.json').write_text(json.dumps(objs, indent=1) + '\n')
print(f'{out.name}: {len(objs)} objects, {failed} failed'); sys.exit(1 if failed else 0)
