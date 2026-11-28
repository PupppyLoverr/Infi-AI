#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""The disk scripts/mk-puredarwin-disk.sh makes, looked at from outside (tests/puredarwin-disk/run.sh).

    check.py --make-root ROOT.tar.gz        a small root file system as a tarball (no root needed: the members' owners
                                            and modes are written into the archive)
    check.py IMG CONF ROOT.tar.gz MOUNT     IMG is the disk, CONF what the script printed, MOUNT the volume mounted
                                            read-only with owners (run as root)

The second form checks what the hosted kernel depends on: the GUID partition table has one Apple HFS+ partition whose
unique GUID is CONF's ROOT_UUID and BOOT_UUID (xnu.ko finds the root by it); the volume is HFS+ with a journal that has
nothing to replay, and was unmounted cleanly (XNU refuses a dirty volume without a journal for good, and would replay
a journal that still holds transactions over what the host wrote); every member of the tarball is on the volume
with its type, mode, owner, group, size, content and link target, a hard link as one file with its target on a Mac
and as a file of its own on Linux; /sbin/fsck is the script's own.
"""
import hashlib
import io
import os
import stat
import struct
import sys
import tarfile
import uuid

HFS_PLUS = uuid.UUID('48465300-0000-11AA-AA11-00306543ECAC')
OWN = {'.journal', '.journal_info_block'}		# the journal's two files (Linux shows them, a Mac hides them)
# Linux's hfsplus writes a hard link without the link chain macOS has kept since 10.5; XNU's hfs then marks the file as
# having one, and fsck finds the volume corrupt after the first boot. The script stores such files as copies there.
LINUX = sys.platform == 'linux'


def make_root(out):
    blob = bytes((i * 7 + (i >> 8)) & 0xff for i in range(1 << 20))
    members = [		# name, type, mode, uid, gid, content or link target
        ('.', 'd', 0o755, 0, 0, None),
        ('./sbin', 'd', 0o755, 0, 0, None),
        ('./bin', 'd', 0o755, 0, 0, None),
        ('./bin/tool', 'f', 0o755, 0, 0, b'#!/bin/sh\necho tool\n'),
        ('./bin/also-tool', 'h', 0o755, 0, 0, './bin/tool'),
        ('./bin/sh', 'l', 0o755, 0, 0, 'tool'),
        ('./usr', 'd', 0o755, 0, 0, None),
        ('./usr/bin', 'd', 0o755, 0, 0, None),
        ('./usr/bin/su', 'f', 0o4755, 0, 0, b'setuid\n'),
        ('./etc', 'd', 0o755, 0, 0, None),
        ('./etc/master.passwd', 'f', 0o600, 0, 0, b'root:*:0:0::0:0:System Administrator:/var/root:/bin/sh\n'),
        ('./private', 'd', 0o755, 0, 0, None),
        ('./private/tmp', 'd', 0o1777, 0, 0, None),
        ('./tmp', 'l', 0o755, 0, 0, 'private/tmp'),
        ('./Users', 'd', 0o755, 0, 80, None),
        ('./Users/someone', 'd', 0o700, 501, 20, None),
        ('./Users/someone/notes.txt', 'f', 0o644, 501, 20, b'owned by 501:20\n'),
        ('./Users/someone/megabyte.bin', 'f', 0o640, 501, 20, blob),
        ('./Users/someone/empty', 'f', 0o644, 501, 20, b''),
        ('./var', 'd', 0o755, 0, 0, None),
        ('./var/empty', 'd', 0o555, 0, 3, None),
        ('./var/daemon-owned', 'f', 0o644, 1, 1, b'uid 1, gid 1\n'),
    ]
    with tarfile.open(out, 'w:gz', format=tarfile.GNU_FORMAT) as tar:
        for name, kind, mode, uid, gid, data in members:
            m = tarfile.TarInfo(name)
            m.mode, m.uid, m.gid, m.mtime = mode, uid, gid, 1700000000
            m.type = {'d': tarfile.DIRTYPE, 'f': tarfile.REGTYPE, 'l': tarfile.SYMTYPE, 'h': tarfile.LNKTYPE}[kind]
            if kind in 'lh':
                m.linkname = data
            if kind == 'f':
                m.size = len(data)
            tar.addfile(m, io.BytesIO(data) if kind == 'f' else None)
    print(f'{out}: {len(members)} members')


def partition(img, conf):
    """The one Apple HFS+ partition of the GUID partition table: its first byte. Its GUID must be CONF's."""
    with open(img, 'rb') as f:
        f.seek(512)
        head = f.read(92)
        if head[:8] != b'EFI PART':
            sys.exit(f'{img}: no GUID partition table')
        lba, count, size = struct.unpack('<QII', head[72:88])
        f.seek(lba * 512)
        entries = [f.read(size) for _ in range(count)]
    hfs = [e for e in entries if uuid.UUID(bytes_le=e[:16]) == HFS_PLUS]
    if len(hfs) != 1:
        sys.exit(f'{img}: {len(hfs)} Apple HFS+ partitions, expected 1')
    guid = str(uuid.UUID(bytes_le=hfs[0][16:32])).upper()
    want = dict(line.split('=', 1) for line in open(conf).read().split())
    if set(want) != {'ROOT_UUID', 'BOOT_UUID'} or want['ROOT_UUID'] != guid or want['BOOT_UUID'] != guid:
        sys.exit(f"the partition's GUID is {guid}, the script printed {want}")
    print(f'partition: Apple HFS+, GUID {guid} = ROOT_UUID = BOOT_UUID')
    return struct.unpack('<Q', hfs[0][32:40])[0] * 512


def volume(img, start):
    with open(img, 'rb') as f:
        f.seek(start + 1024)
        head = f.read(512)
        sig, _, attr = struct.unpack('>2sHI', head[:8])
        info_block, = struct.unpack('>I', head[12:16])
        block_size, = struct.unpack('>I', head[40:44])
        if sig != b'H+':
            sys.exit(f'the volume is not HFS+ (signature {sig!r})')
        if not attr & 0x2000 or not info_block:
            sys.exit(f'the volume has no journal (attributes {attr:#x}): XNU refuses it for good after the first '
                     'run that ends without an unmount')
        if not attr & 0x100 or attr & 0x800:
            sys.exit(f'the volume was not unmounted cleanly, or is marked inconsistent (attributes {attr:#x})')
        f.seek(start + info_block * block_size)
        flags, = struct.unpack('>I', f.read(4))
        f.seek(start + info_block * block_size + 36)
        offset, jsize = struct.unpack('>QQ', f.read(16))
        if flags & 4:		# kJIJournalNeedInitMask: the first mount writes the journal's header
            state = 'to be initialised by the first mount'
        else:
            f.seek(start + offset)
            j = f.read(32)
            order = '<' if j[:4] == b'xLNJ' else '>'
            if j[:4] not in (b'xLNJ', b'JNLx'):
                sys.exit(f'the journal has no header (magic {j[:4]!r}) and is not marked for initialisation')
            first, last = struct.unpack(order + 'QQ', j[8:24])
            if first != last:
                sys.exit(f'the journal still holds transactions ({first:#x} to {last:#x}): XNU would replay them '
                         'over the files written since')
            state = 'empty'
    print(f'volume: HFS+, journal of {jsize >> 10} kB {state}, unmounted cleanly (attributes {attr:#x})')


def tree(root_tar, mount):
    tar = tarfile.open(root_tar)
    names, inode, bad, n = set(), {}, 0, 0
    for m in tar:
        name = m.name[2:] if m.name.startswith('./') else m.name
        if name in ('', '.'):
            continue
        names.add(name)
        if name == 'sbin/fsck':
            continue
        n += 1
        p = os.path.join(mount, name)
        try:
            st = os.lstat(p)
        except OSError as e:
            bad += 1
            print('missing:', name, e.strerror)
            continue
        want = [stat.S_IMODE(m.mode), m.uid, m.gid]
        have = [stat.S_IMODE(st.st_mode), st.st_uid, st.st_gid]
        if m.issym():		# a link's own mode is the file system's business
            kind = stat.S_ISLNK(st.st_mode)
            want, have = want[1:] + [m.linkname], have[1:] + [os.readlink(p) if kind else None]
        elif m.isdir():
            kind = stat.S_ISDIR(st.st_mode)
        elif m.islnk():		# a hard link: one file with its target on a Mac, a file of its own on Linux (LINKED)
            kind = stat.S_ISREG(st.st_mode)
            target = m.linkname[2:] if m.linkname.startswith('./') else m.linkname
            data = tar.extractfile(tar.getmember(m.linkname)).read()
            want += [len(data), hashlib.sha256(data).hexdigest(), None if LINUX else inode.get(target)]
            have += [st.st_size, hashlib.sha256(open(p, 'rb').read()).hexdigest() if kind else None, None if LINUX else st.st_ino]
        else:
            kind = stat.S_ISREG(st.st_mode)
            inode[name] = st.st_ino
            want += [m.size, hashlib.sha256(tar.extractfile(m).read()).hexdigest()]
            have += [st.st_size, hashlib.sha256(open(p, 'rb').read()).hexdigest() if kind else None]
        if not kind or want != have:
            bad += 1
            print('differs:', name, 'want', want, 'have', have)
    extra, linked = [], []
    for d, dirs, files in os.walk(mount):
        for f in dirs + files:
            rel = os.path.relpath(os.path.join(d, f), mount)
            if rel not in names and rel not in OWN and rel != 'sbin/fsck':
                extra.append(rel)
            st = os.lstat(os.path.join(d, f))
            if LINUX and stat.S_ISREG(st.st_mode) and st.st_nlink > 1:
                linked.append(rel)
    if linked:
        bad += 1
        print('hard links written by Linux (no link chain: fsck fails after the first boot):', sorted(linked))
    if extra:
        bad += 1
        print('on the disk and not in the tarball:', sorted(extra))
    fsck = os.path.join(mount, 'sbin/fsck')
    st = os.lstat(fsck) if os.path.lexists(fsck) else None
    if not st or not stat.S_ISREG(st.st_mode) or [stat.S_IMODE(st.st_mode), st.st_uid, st.st_gid] != [0o755, 0, 0] \
            or '-o update,rw' not in open(fsck).read():
        bad += 1
        print('/sbin/fsck is not the remount script, 0755 and root:wheel:',
              [oct(st.st_mode), st.st_uid, st.st_gid] if st else 'it is not there')
    print(f'tree: {n} members compared, {bad} wrong')
    return bad


if sys.argv[1] == '--make-root':
    make_root(sys.argv[2])
else:
    img, conf, root_tar, mount = sys.argv[1:5]
    volume(img, partition(img, conf))
    sys.exit(1 if tree(root_tar, mount) else 0)
