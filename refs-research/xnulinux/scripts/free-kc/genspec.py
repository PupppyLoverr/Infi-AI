#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Native kext descriptions (scripts/kclink/kclink.py --native) for the free kexts that kcc.py built: bundle, version, libraries and
personalities from the kext's own Info.plist, its objects plus the compat objects it needs, and its kmod start/stop.

    genspec.py SRC OBJDIR SPECDIR NAME...

The collection they are linked into (mk-template.py) has no kexts, so none stands in for another. The com.apple.kec.*
ones are marked AppleKernelExternalComponent, which is what makes KLDBootstrap start them before IOKit
(corecrypto must register before IOPlatformExpertDevice hashes the platform UUID).
"""
import json, plistlib, sys
from pathlib import Path

SRC, O, OUT = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
OUT.mkdir(parents=True, exist_ok=True)
KEXTS = {   # name: (Info.plist below SRC, bundle id when the plist leaves it to the build, start, stop, compat objects)
    'corecrypto': (str(Path(__file__).resolve().parents[2] / 'kexts/free/corecrypto/Info.plist'), None, '_corecrypto_start',
                   '_corecrypto_stop', []),
    'amfi': ('pd/src/Kernel/Extensions/amfi/Info.plist', None, '_amfi_kext_start', '_amfi_kext_stop', ['kalloc-type']),
    'pthread': ('pd/src/Kernel/Extensions/pthread/pthread-Info.plist', None, '_pthread_start', '_pthread_stop', ['kalloc-type']),
    'IOStorageFamily': ('pd/src/Kernel/Extensions/IOStorageFamily/Info.plist', None, None, None, ['osmalloc']),
    'HFSEncodings': ('pd/src/Kernel/Extensions/HFSEncodings/Info.plist', None, None, None, []),
    'hfs': ('pd/src/Kernel/Extensions/hfs/Info.plist', None, None, None, ['loadtag']),
}
for name in sys.argv[4:]:
    plist, bundle, start, stop, compat = KEXTS[name]
    info = plistlib.load(open(SRC / plist, 'rb'))
    info.setdefault('CFBundleIdentifier', bundle)
    objects = json.loads((O / name / 'objects.json').read_text())
    for c in compat:
        objects += json.loads((O / 'compat' / c / 'objects.json').read_text())
    if not start:   # no kmod start routine of its own: the shared one (kexts/free/compat/kmod-start.c)
        objects += json.loads((O / 'compat' / 'kmod-start' / 'objects.json').read_text())
        start, stop = '_native_kext_start', '_native_kext_stop'
    (O / name / 'objects-kc.json').write_text(json.dumps(objects, indent=1) + '\n')
    spec = {'name': name, 'bundle': info['CFBundleIdentifier'],
            # kclink gives a native kext OSBundleCompatibleVersion 1.0.0, which must not exceed its version (pthread's 1.0.0d1)
            'version': str(info.get('CFBundleVersion', '1.0')).split('d')[0],
            'objects': str(O / name / 'objects-kc.json'), 'start': start, 'stop': stop,
            'libraries': info.get('OSBundleLibraries', {}), 'personalities': info.get('IOKitPersonalities', {}),
            'plist': {'AppleKernelExternalComponent': True} if info['CFBundleIdentifier'].startswith('com.apple.kec.') else {}}
    (OUT / f'{name}.json').write_text(json.dumps(spec, indent=1, default=str) + '\n')
    print(f'{name}: {spec["bundle"]}, {len(objects)} objects, {len(spec["personalities"])} personalities')
