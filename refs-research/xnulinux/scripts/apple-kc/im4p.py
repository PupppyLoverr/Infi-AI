#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Unpack a kernelcache for scripts/apple-kc/link.sh, with python3 alone (and lzfse for an LZFSE payload):

    im4p.py INPUT OUTPUT [MEMBER]

INPUT is an IPSW (a zip: MEMBER, default kernelcache.release.vma2, is read from it), an IPSW's URL (Apple's own:
only the member is downloaded, with HTTP range requests), an IM4P, an IMG4 with the IM4P in it (the kernelcache file of
an installed macOS, on its Preboot volume), or an unpacked Mach-O (copied). An IM4P is DER: SEQUENCE { "IM4P", type,
description, OCTET STRING payload, ... }; the payload is LZFSE ("bvx2...", decoded by the lzfse tool: apt install
lzfse), LZSS ("complzss" with Apple's 0x180-byte header), or plain.

A file that is cut short or does not unpack to what it says is refused, and OUTPUT is not written: the DER lengths
must fit the file, the unpacked size must be the one the IM4P (LZFSE) or the LZSS header states, and the result must
be a Mach-O.
"""
import http.client
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile


MACHO = b'\xcf\xfa\xed\xfe'


class Remote:
    """A read-only, seekable file over HTTP range requests (what zipfile needs to read one member of a remote zip).
    Small reads (zip records) go through a cache of 256 KiB blocks; large ones (the member) are fetched as they are, in
    16 MiB pieces. A piece whose transfer breaks off is asked for again (a few times): a download over a long path does."""
    BLOCK, PIECE, TRIES = 1 << 18, 1 << 24, 6

    def __init__(self, url):
        self.url, self.pos, self.cache = url, 0, {}
        with urllib.request.urlopen(urllib.request.Request(url, method='HEAD'), timeout=60) as r:
            self.size = int(r.headers['Content-Length'])

    def seekable(self):
        return True

    def tell(self):
        return self.pos

    def seek(self, offset, whence=0):
        self.pos = offset if whence == 0 else self.pos + offset if whence == 1 else self.size + offset
        return self.pos

    def fetch(self, start, end):
        """Bytes start..end (inclusive)."""
        for attempt in range(self.TRIES):
            try:
                req = urllib.request.Request(self.url, headers={'Range': f'bytes={start}-{end}', 'Accept-Encoding': 'identity'})
                with urllib.request.urlopen(req, timeout=120) as r:
                    if r.status != 206:
                        raise SystemExit(f'{self.url}: the server does not serve byte ranges')
                    data = r.read()
                if len(data) == end - start + 1:
                    return data
                error = f'{len(data)} of {end - start + 1} bytes'
            except (OSError, http.client.HTTPException) as e:
                error = e
            print(f'bytes {start}-{end}: {error}; again', file=sys.stderr)
            time.sleep(2 ** attempt)
        raise SystemExit(f'{self.url}: bytes {start}-{end} could not be read')

    def read(self, n=-1):
        if n < 0:
            n = self.size - self.pos
        n = max(0, min(n, self.size - self.pos))
        out = bytearray()
        if n > self.BLOCK:
            while len(out) < n:
                k = min(self.PIECE, n - len(out))
                out += self.fetch(self.pos, self.pos + k - 1)
                self.pos += k
            return bytes(out)
        while len(out) < n:
            i = self.pos // self.BLOCK
            if i not in self.cache:
                self.cache[i] = self.fetch(i * self.BLOCK, min((i + 1) * self.BLOCK, self.size) - 1)
            k = self.pos % self.BLOCK
            piece = self.cache[i][k:k + n - len(out)]
            out += piece
            self.pos += len(piece)
        return bytes(out)


def der(data, at):
    """(tag, start of contents, end of contents) of the DER element at `at`. Its contents must be there: a file
    that was cut short (a download that stopped) ends before one of its elements does."""
    if at + 2 > len(data):
        raise SystemExit(f'cut short: {len(data)} bytes, and a DER element starts at {at}')
    tag, n = data[at], data[at + 1]
    at += 2
    if n & 0x80:
        k = n & 0x7f
        n = int.from_bytes(data[at:at + k], 'big')
        at += k
    if at + n > len(data):
        raise SystemExit(f'cut short: {len(data)} bytes, and a DER element of it ends at {at + n}')
    return tag, at, at + n


def elements(data, at, end):
    """The DER elements in data[at:end], as der() gives them."""
    items = []
    while at < end:
        t, s, e = der(data, at)
        items.append((t, s, e))
        at = e
    return items


def payload(data):
    """(payload, its unpacked size or None) of an IM4P, or of the IM4P in an IMG4 (SEQUENCE { "IMG4", IM4P, manifest }).
    The size is the IM4P's compression record, SEQUENCE { INTEGER algorithm, INTEGER unpacked size }, where it has one
    (Apple's LZFSE kernelcaches do)."""
    tag, at, end = der(data, 0)
    if tag != 0x30:
        raise SystemExit('not an IM4P (no DER SEQUENCE)')
    items = elements(data, at, end)
    if len(items) > 1 and data[items[0][1]:items[0][2]] == b'IMG4' and items[1][0] == 0x30:
        items = elements(data, items[1][1], items[1][2])
    if len(items) < 4 or data[items[0][1]:items[0][2]] != b'IM4P' or items[3][0] != 0x04:
        raise SystemExit('not an IM4P')
    size = None
    for t, s, e in items[4:]:
        sub = elements(data, s, e) if t == 0x30 else []
        if len(sub) == 2 and sub[0][0] == sub[1][0] == 0x02:
            size = int.from_bytes(data[sub[1][1]:sub[1][2]], 'big')
    return data[items[3][1]:items[3][2]], size


def lzss(src, size):
    """Apple's kernelcache LZSS (4096-byte window, 18-byte matches, threshold 2)."""
    N, F, T = 4096, 18, 2
    text = bytearray(b' ' * (N - F)) + bytearray(F)
    r, out, i, flags = N - F, bytearray(), 0, 0
    while i < len(src) and len(out) < size:
        flags >>= 1
        if not flags & 0x100:
            flags = src[i] | 0xff00
            i += 1
            if i >= len(src):
                break
        if flags & 1:
            c = src[i]
            i += 1
            out.append(c)
            text[r] = c
            r = (r + 1) & (N - 1)
        else:
            if i + 1 >= len(src):
                break
            p, q = src[i], src[i + 1]
            i += 2
            p |= (q & 0xf0) << 4
            for k in range((q & 0x0f) + T + 1):
                c = text[(p + k) & (N - 1)]
                out.append(c)
                text[r] = c
                r = (r + 1) & (N - 1)
    return bytes(out[:size])


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    src, dst = sys.argv[1], sys.argv[2]
    member = sys.argv[3] if len(sys.argv) > 3 else 'kernelcache.release.vma2'
    if src.startswith(('https://', 'http://')):
        print(f'reading {member} from {src}', file=sys.stderr)
        with zipfile.ZipFile(Remote(src)) as z:
            data = z.read(member)
    elif zipfile.is_zipfile(src):
        with zipfile.ZipFile(src) as z:
            data = z.read(member)
    else:
        data = open(src, 'rb').read()
    if data[:4] == MACHO:
        open(dst, 'wb').write(data)
        return
    body, size = payload(data)
    if body[:4] == b'bvx2':
        lz = shutil.which('lzfse')
        if not lz:
            raise SystemExit('the kernelcache is LZFSE-compressed: install lzfse (apt install lzfse)')
        # beside OUTPUT, moved there once it is known to be whole
        with tempfile.NamedTemporaryFile() as f, tempfile.TemporaryDirectory(dir=os.path.dirname(os.path.abspath(dst))) as d:
            f.write(body)
            f.flush()
            out = os.path.join(d, 'kernelcache')
            if subprocess.run([lz, '-decode', '-i', f.name, '-o', out]).returncode != 0 or not os.path.exists(out):
                raise SystemExit(f'{src}: lzfse could not decode the payload (damaged?)')
            have = os.path.getsize(out)
            if size is not None and have != size:
                raise SystemExit(f'{src}: the payload unpacks to {have} bytes, the IM4P says {size} (damaged?)')
            if open(out, 'rb').read(4) != MACHO:
                raise SystemExit(f'{src}: the payload does not unpack to a Mach-O (damaged?)')
            os.replace(out, dst)
        return
    if body[:8] == b'complzss':
        if len(body) < 0x180:
            raise SystemExit(f'{src}: cut short (an LZSS payload of {len(body)} bytes, less than its header)')
        size, = struct.unpack('>I', body[12:16])
        body = lzss(body[0x180:], size)
        if len(body) != size:
            raise SystemExit(f'{src}: the payload unpacks to {len(body)} bytes, its LZSS header says {size} (cut short?)')
    if body[:4] != MACHO:
        raise SystemExit(f'{src}: the IM4P payload is not a kernelcache ({body[:8]!r})')
    open(dst, 'wb').write(body)


if __name__ == '__main__':
    main()
