# SPDX-License-Identifier: GPL-2.0-only
"""A kernel collection to test with, made the way scripts/free-kc/build.sh makes one (mk-template.py's empty collection,
then kclink.py) from stand-ins: a kernel of a few pages with threaded rebases and four symbols, kexts from objects
built by mkmacho.py, made-up property lists for XNU's pseudo-extensions. And a reader of the result."""
import json
import os
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile

import mkmacho
from mkmacho import (BL, BRANCH26, GOT_LOAD_PAGE21, GOT_LOAD_PAGEOFF12, KEY_DA, KEY_IA, LC_FUNCTION_STARTS, PAGE21,
                     PAGEOFF12, RET, S_CSTRING_LITERALS, S_ZEROFILL, TEXT_FLAGS, Object, add, adrp, movz, words)

REPO = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..')
MK_TEMPLATE = os.path.join(REPO, 'scripts', 'free-kc', 'mk-template.py')
KCLINK = os.path.join(REPO, 'scripts', 'kclink', 'kclink.py')
KCCOMPARE = os.path.join(REPO, 'scripts', 'kclink', 'kccompare.py')
BASE = 0xfffffff007004000        # where mk-template.py's collection is
PAGE = 0x4000
NO_ADDRESS = 0x7fffffffffffffff
# a collection of 24 pages, laid out like scripts/free-kc/layout.json
LAYOUT = {'segments': [['__TEXT', '0x0', '0x4000', 1, None], ['__PRELINK_TEXT', '0x4000', '0x1c000', 1, '__text'],
                       ['__DATA_CONST', '0x20000', '0x10000', 1, None], ['__TEXT_EXEC', '0x30000', '0x10000', 5, None],
                       ['__PRELINK_INFO', '0x40000', '0x8000', 3, '__info'], ['__DATA', '0x48000', '0x10000', 3, None],
                       ['__LINKEDIT', '0x58000', '0x8000', 1, None]],
          'kernel_header': '0x4000', 'kernel_linkinfo': ['0x58000', '0x4000'], 'kernel_linkedit': '0x60000'}
KPI = {'BSDKernel': 'com.apple.kpi.bsd', 'MACFramework': 'com.apple.kpi.dsep', 'IOKit': 'com.apple.kpi.iokit',
       'Libkern': 'com.apple.kpi.libkern', 'Mach': 'com.apple.kpi.mach', 'Private': 'com.apple.kpi.private',
       'Unsupported': 'com.apple.kpi.unsupported'}
VERSION = '27.2.0'


def region(layout, name):
    """(offset, size) of one of the collection's segments."""
    return next((int(off, 16), int(size, 16)) for n, off, size, prot, sect in layout['segments'] if n == name)


def stand_in_kernel(layout=LAYOUT, base=BASE, data_at=0, prelink_data=None, more=()):
    """A kernel for a collection of this layout whose header is at base: each segment at the start of its region.
    data_at: where in its data _kernel_data is; prelink_data: the offset in the collection of an empty __PRELINK_DATA
    segment (where a kernel has its kexts' data start); more: further segments, (name, offset in the collection).
    Returns (the file, {symbol: address}, [(address of a pointer, its target, how it is signed)])."""
    regions = ('__PRELINK_TEXT', '__DATA_CONST', '__TEXT_EXEC', '__DATA')
    text, dc, exec_, data = (base + region(layout, n)[0] for n in regions)
    info = base + int(layout['kernel_linkinfo'][0], 16)
    symbols = [('_kernel_start', exec_, True), ('_kernel_func', exec_ + 4, True),
               ('_kernel_private', exec_ + 12, False), ('_kernel_data', data + data_at, True)]
    # threaded rebases: two chains, one with its last pointer over the end of a page
    chains = [[(dc, exec_ + 4, None), (dc + 8, exec_ + 4, (KEY_IA, 0x1234, 1))],
              [(dc + 0x3ffc, data, None), (dc + 0x4008, data + 8, (KEY_DA, 0, 0))]]
    segs = [('__TEXT', text, bytes(2 * PAGE), [('__const', 0x2000, 0x100, 0)]),
            ('__DATA_CONST', dc, bytes(2 * PAGE), [('__const', 0, 2 * PAGE, 0)]),
            ('__TEXT_EXEC', exec_, words(RET, movz(0, 7), RET, RET) + bytes(PAGE - 16),
             [('__text', 0, 16, TEXT_FLAGS)]),
            ('__DATA', data, struct.pack('<Q', 0x1122334455667788) + bytes(PAGE - 8),
             [('__data', 0, 0x100, 0), ('__bss', 0x100, 0x200, S_ZEROFILL)])]
    if prelink_data is not None:
        segs.append(('__PRELINK_DATA', base + prelink_data, b'', []))
    segs += [(name, base + off, bytes(PAGE), []) for name, off in more]
    segs.append(('__LINKINFO', info, b'link info' + bytes(PAGE - 9), []))
    buf = mkmacho.kernel(segs, symbols, chains, extra=[struct.pack('<4I', LC_FUNCTION_STARTS, 16, 0, 0)])
    return buf, {n: a for n, a, ext in symbols}, [p for chain in chains for p in chain]


def kext_one(answer=9, diversity=0x1111, message=b'hello from a kext\0'):
    """Two objects of a kext that calls the kernel, loads the address of its data through a GOT and has pointers to
    the kernel, to itself (signed) and to a symbol of the kernel that is not exported."""
    one, two = Object(), Object()
    t = one.text(BL, BL, adrp(1), 0xf9400021, adrp(1), add(1, 1), RET, RET)
    d = one.section('__DATA', '__data', bytes(24), 3)
    one.section('__DATA', '__bss', align_=4, zerofill=64)
    s = {n: one.undefined(n) for n in ('_helper', '_kernel_func', '_kernel_data', '_message')}
    start = one.define('_one_start', t)
    one.define('_one_stop', t, 28)
    one.define('_pointers', d)
    one.reloc(t, 0, BRANCH26, s['_helper'], pcrel=1)
    one.reloc(t, 4, BRANCH26, s['_kernel_func'], pcrel=1)
    one.reloc(t, 8, GOT_LOAD_PAGE21, s['_kernel_data'], pcrel=1)
    one.reloc(t, 12, GOT_LOAD_PAGEOFF12, s['_kernel_data'])
    one.reloc(t, 16, PAGE21, s['_message'], pcrel=1)
    one.reloc(t, 20, PAGEOFF12, s['_message'])
    one.pointer(d, 0, s['_kernel_data'])
    one.auth_pointer(d, 8, start, KEY_IA, diversity, 1)
    one.pointer(d, 16, s['_message'])
    helper = two.define('_helper', two.text(movz(0, answer), RET))
    two.define('_message', two.section('__TEXT', '__cstring', message, 0, S_CSTRING_LITERALS))
    c = two.section('__DATA', '__const', bytes(16), 3)
    two.define('_table', c)
    two.auth_pointer(c, 0, helper, KEY_IA, 0)
    two.pointer(c, 8, two.undefined('_kernel_private'))
    return [one, two]


def kext_two():
    """A kext that calls the first one's _helper."""
    o = Object()
    t = o.text(BL, RET, RET)
    o.define('_two_start', t)
    o.define('_two_stop', t, 8)
    o.reloc(t, 0, BRANCH26, o.undefined('_helper'), pcrel=1)
    return [o]


def kext_three(pointer_to='_helper'):
    """A kext with a pointer to a symbol of the first one (and no word of it among its libraries)."""
    o = Object()
    t = o.text(RET, RET)
    d = o.section('__DATA', '__data', bytes(8), 3)
    o.define('_three_start', t)
    o.define('_three_stop', t, 4)
    o.define('_three_pointer', d)
    o.pointer(d, 0, o.undefined(pointer_to))
    return [o]


SPEC_ONE = {'name': 'One', 'bundle': 'org.example.kext.one', 'version': '1.0.0', 'start': '_one_start',
            'stop': '_one_stop', 'libraries': {'com.apple.kpi.libkern': '8.0'},
            'personalities': {'One': {'IOClass': 'One', 'IOProviderClass': 'IOResources'}}}
SPEC_TWO = {'name': 'Two', 'bundle': 'org.example.kext.two', 'version': '2.0', 'start': '_two_start',
            'stop': '_two_stop', 'libraries': {'org.example.kext.one': '1.0'}}


SPEC_THREE = {'name': 'Three', 'bundle': 'org.example.kext.three', 'version': '3', 'start': '_three_start',
              'stop': '_three_stop'}


class Workdir:
    """A directory to build collections in: XNU's pseudo-extension property lists (made up), a layout."""

    def __init__(self, cleanup, layout=LAYOUT, env=None):
        self.dir = tempfile.mkdtemp()
        cleanup(shutil.rmtree, self.dir)
        self.layout, self.env = layout, dict(os.environ, **(env or {}))
        for bundle, ident in KPI.items():
            d = self.path('xnu', 'config', 'System.kext', 'PlugIns', bundle + '.kext')
            os.makedirs(d)
            with open(os.path.join(d, 'Info.plist'), 'wb') as f:
                plistlib.dump({'CFBundleIdentifier': ident, 'CFBundleName': bundle, 'OSKernelResource': True,
                               'CFBundleVersion': '###KERNEL_VERSION_LONG###', 'OSBundleCompatibleVersion': '8.0.0b1',
                               'CFBundleShortVersionString': '###KERNEL_VERSION_SHORT###'}, f)
        with open(self.path('layout.json'), 'w') as f:
            json.dump(layout, f)

    def path(self, *parts):
        return os.path.join(self.dir, *parts)

    def read(self, name):
        with open(self.path(name), 'rb') as f:
            return f.read()

    def write(self, name, content):
        with open(self.path(name), 'wb' if isinstance(content, bytes) else 'w') as f:
            f.write(content)
        return name

    def run(self, script, *args):
        """Runs one of the scripts in the directory; returns (exit status, what it printed, what it complained of)."""
        r = subprocess.run([sys.executable, script] + list(args), cwd=self.dir, capture_output=True, text=True,
                           env=self.env)
        return r.returncode, r.stdout, r.stderr

    def template(self, name='template.kc', version=VERSION, layout='layout.json'):
        status, out, err = self.run(MK_TEMPLATE, layout, 'xnu', version, name)
        assert status == 0, err
        return self.read(name)

    def kernel(self, name='kernel', base=BASE, **how):
        buf, symbols, pointers = stand_in_kernel(self.layout, base, **how)
        self.write(name, buf)
        return symbols, pointers

    def native(self, spec, objects):
        """Writes a kext's objects and its spec; returns the spec's name."""
        stem = spec['name'].lower()
        paths = [o.write(self.dir, f'{stem}{i}.o') for i, o in enumerate(objects)]
        self.write(stem + '.objects.json', json.dumps([os.path.basename(p) for p in paths]))
        return self.write(stem + '.json', json.dumps(dict(spec, objects=stem + '.objects.json')))

    def link(self, *args, base='template.kc', kernel='kernel', output='out.kc'):
        return self.run(KCLINK, base, kernel, output, *args)

    def collection(self, name='out.kc'):
        """The collection of this name, read back, with the symbols of its kexts kclink wrote beside it."""
        return Collection(self.read(name), json.loads(self.read(os.path.splitext(name)[0] + '.natives.json')))


class Collection:
    """A collection read back: its segments, its entries, its chained fixups, its prelink info."""

    def __init__(self, buf, natives=None):
        self.buf = buf
        self.segments = {s['name']: s for s in mkmacho.segments(buf)}
        self.base = self.segments['__TEXT']['vmaddr']
        self.entries = mkmacho.fileset_entries(buf)
        self.header, self.starts, self.pointers = mkmacho.chained_fixups(buf)
        info = self.segments['__PRELINK_INFO']
        self.plist = buf[info['fileoff']:info['fileoff'] + info['sections'][0]['size']]
        self.info = plistlib.loads(self.plist)
        self.kexts = {d['CFBundleIdentifier']: d for d in self.info['_PrelinkInfoDictionary']}
        self.natives = natives or {}

    def u32(self, va):
        return struct.unpack_from('<I', self.buf, va - self.base)[0]

    def pointer(self, va):
        """(target, signing) of the chained fixup at an address."""
        p = self.pointers[va - self.base]
        return self.base + p.target, p.signing()

    def entry(self, name):
        """(the offset of an entry's header, its segments by name)."""
        off = self.entries[name][1]
        return off, {s['name']: s for s in mkmacho.segments(self.buf, off)}
