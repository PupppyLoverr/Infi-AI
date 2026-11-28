#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""scripts/apple-kc/im4p.py on kernelcaches made here (nothing of Apple's): every input it takes unpacks to the kernel
that went in (a Mach-O, an IM4P that is plain, LZSS or LZFSE, the IMG4 around one, an IPSW, an IPSW's URL read with range
requests), and a file that is cut short or does not unpack to what it says is refused with OUTPUT not written.

    python3 tests/apple-kc/test_im4p.py [-v]

The LZFSE round trip needs the lzfse tool (it packs the test's kernel too) and is skipped without it, unless
IM4P_TEST_NEED_LZFSE is set (CI), where a missing tool fails the test.
"""
import http.server
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import unittest
import zipfile
import zlib
from pathlib import Path

IM4P = Path(__file__).resolve().parents[2] / 'scripts/apple-kc/im4p.py'
MACHO = b'\xcf\xfa\xed\xfe'
SMALL = MACHO + bytes(range(256)) * 60 + b'xnulinux ' * 3000 + bytes(4000)   # 46 KB: literals, matches, a long run
BIG = MACHO + bytes((i * 7 + (i >> 8)) & 0xff for i in range(400000))        # more than one of Remote's cache blocks


def tlv(tag, body):
    n = len(body)
    if n < 0x80:
        return bytes([tag, n]) + body
    b = n.to_bytes((n.bit_length() + 7) // 8, 'big')
    return bytes([tag, 0x80 | len(b)]) + b + body


def integer(v):
    return tlv(0x02, v.to_bytes(v.bit_length() // 8 + 1, 'big'))


def im4p(payload, size=None):
    """SEQUENCE { "IM4P", type, description, OCTET STRING payload [, SEQUENCE { INTEGER 1, INTEGER unpacked size }] }"""
    record = tlv(0x30, integer(1) + integer(size)) if size is not None else b''
    return tlv(0x30, tlv(0x16, b'IM4P') + tlv(0x16, b'krnl') + tlv(0x16, b'a test kernelcache') + tlv(0x04, payload) + record)


def img4(inner):
    """SEQUENCE { "IMG4", IM4P, [0] manifest }: the kernelcache file of an installed macOS"""
    return tlv(0x30, tlv(0x16, b'IMG4') + inner + tlv(0xa0, tlv(0x30, tlv(0x16, b'IM4M') + bytes(300))))


def lzss_pack(data):
    """Apple's kernelcache LZSS (4096-byte ring written from 4078 on, matches of 3 to 18 bytes), with its header."""
    out, i, last = bytearray(), 0, {}
    while i < len(data):
        flags, group = 0, bytearray()
        for bit in range(8):
            if i >= len(data):
                break
            j, n = last.get(data[i:i + 3], -1), 0
            if j >= 0 and i - j <= 4000:
                while n < 18 and i + n < len(data) and data[j + n] == data[i + n]:
                    n += 1
            if n >= 3:
                at = (4078 + j) & 4095
                group += bytes([at & 0xff, ((at >> 4) & 0xf0) | (n - 3)])
            else:
                n = 1
                flags |= 1 << bit
                group.append(data[i])
            for k in range(i, i + n):
                last[data[k:k + 3]] = k
            i += n
        out.append(flags)
        out += group
    return b'complzss' + struct.pack('>III', zlib.adler32(data), len(data), len(out)) + bytes(0x180 - 20) + bytes(out)


class Ranges(http.server.BaseHTTPRequestHandler):
    """One file, with byte ranges. server.mode: 'cut' breaks the first transfer of more than 64 KiB off halfway,
    'whole' ignores Range (200 and the whole file)."""

    def log_message(self, *args):
        pass

    def do_HEAD(self):
        self.send_response(200)
        self.send_header('Content-Length', str(len(self.server.blob)))
        self.end_headers()

    def do_GET(self):
        try:
            self.answer()
        except (BrokenPipeError, ConnectionResetError):   # the client had enough (a 200 where it asked for a range)
            pass

    def answer(self):
        blob, rng = self.server.blob, self.headers.get('Range')
        if rng is None or self.server.mode == 'whole':
            self.send_response(200)
            self.send_header('Content-Length', str(len(blob)))
            self.end_headers()
            self.wfile.write(blob)
            return
        a, b = (int(x) for x in rng.split('=')[1].split('-'))
        piece = blob[a:b + 1]
        self.send_response(206)
        self.send_header('Content-Range', f'bytes {a}-{b}/{len(blob)}')
        self.send_header('Content-Length', str(len(piece)))
        self.end_headers()
        if self.server.mode == 'cut' and len(piece) > 1 << 16:
            self.server.mode = None
            piece = piece[:len(piece) // 2]
        self.server.served += len(piece)
        self.wfile.write(piece)


class Im4p(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.d = Path(self.dir.name)
        self.out = self.d / 'out' / 'kernelcache'
        self.out.parent.mkdir()
        self.env = {k: v for k, v in os.environ.items() if not k.lower().endswith('_proxy')}

    def tearDown(self):
        self.dir.cleanup()

    def run_im4p(self, src, *more, path=None):
        env = dict(self.env, PATH=f'{path}{os.pathsep}{self.env["PATH"]}') if path else self.env
        return subprocess.run([sys.executable, str(IM4P), str(src), str(self.out), *more], capture_output=True, text=True, env=env)

    def file(self, name, blob):
        (self.d / name).write_bytes(blob)
        return self.d / name

    def unpacks(self, src, kernel, *more, path=None):
        p = self.run_im4p(src, *more, path=path)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(self.out.read_bytes(), kernel)

    def refused(self, src, why, *more, path=None):
        self.out.unlink(missing_ok=True)
        p = self.run_im4p(src, *more, path=path)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn(why, p.stderr)
        self.assertNotIn('Traceback', p.stderr)
        self.assertEqual(os.listdir(self.out.parent), [], 'nothing is written (and nothing left) beside OUTPUT')

    def test_what_it_takes(self):
        for name, blob in (('macho', SMALL), ('plain.im4p', im4p(SMALL)), ('lzss.im4p', im4p(lzss_pack(SMALL))),
                           ('plain.img4', img4(im4p(SMALL))), ('lzss.img4', img4(im4p(lzss_pack(SMALL))))):
            with self.subTest(name):
                self.unpacks(self.file(name, blob), SMALL)

    def test_lzss_has_matches(self):
        self.assertLess(len(lzss_pack(SMALL)), len(SMALL) // 2)   # the round trip above went through the match path

    def test_ipsw(self):
        ipsw = self.d / 'test.ipsw'
        with zipfile.ZipFile(ipsw, 'w') as z:
            z.writestr('kernelcache.release.vma2', im4p(lzss_pack(SMALL)))
            z.writestr('other/kernelcache', im4p(SMALL[:5000]))
        self.unpacks(ipsw, SMALL)
        self.unpacks(ipsw, SMALL[:5000], 'other/kernelcache')

    def test_cut_short(self):
        for name, blob in (('plain', im4p(SMALL)), ('lzss', im4p(lzss_pack(SMALL))), ('img4', img4(im4p(SMALL)))):
            for keep in (1, 3, 40, len(blob) // 2, len(blob) - 1):
                with self.subTest(name, keep=keep):
                    self.refused(self.file('cut.im4p', blob[:keep]), 'cut short')

    def test_lzss_stream_ends_early(self):
        packed = lzss_pack(SMALL)
        self.refused(self.file('early.im4p', im4p(packed[:len(packed) // 2])), 'its LZSS header says')
        self.refused(self.file('header.im4p', im4p(packed[:0x100])), 'cut short')

    def test_not_a_kernelcache(self):
        self.refused(self.file('empty', b''), 'cut short')
        self.refused(self.file('text', b'<html>503 Service Unavailable</html>\n' * 4), 'not an IM4P')
        self.refused(self.file('other.im4p', im4p(b'not a Mach-O at all, whatever it is')), 'not a kernelcache')
        self.refused(self.file('lzss-other.im4p', im4p(lzss_pack(b'packed, and still not a Mach-O' * 9))), 'not a kernelcache')

    def fake_lzfse(self, script):
        """A directory with an `lzfse` that does what the script says with its -i and -o arguments."""
        tool = self.d / 'bin' / 'lzfse'
        tool.parent.mkdir(exist_ok=True)
        tool.write_text(f'#!{sys.executable}\nimport sys\na = sys.argv\nsrc, dst = a[a.index("-i") + 1], a[a.index("-o") + 1]\n{script}\n')
        tool.chmod(0o755)
        return tool.parent

    def test_lzfse_result_is_checked(self):
        body = b'bvx2' + SMALL   # what the stand-in tools "decode": everything after the magic
        whole = self.fake_lzfse('open(dst, "wb").write(open(src, "rb").read()[4:])')
        self.unpacks(self.file('ok.im4p', im4p(body, len(SMALL))), SMALL, path=whole)
        self.unpacks(self.file('nosize.im4p', im4p(body)), SMALL, path=whole)
        self.unpacks(self.file('ok.img4', img4(im4p(body, len(SMALL)))), SMALL, path=whole)
        self.refused(self.file('size.im4p', im4p(body, len(SMALL) + 512)), 'the IM4P says', path=whole)
        self.refused(self.file('other.im4p', im4p(b'bvx2' + b'no Mach-O in here' * 40)), 'does not unpack to a Mach-O', path=whole)
        short = self.fake_lzfse('open(dst, "wb").write(open(src, "rb").read()[4:1000])')
        self.refused(self.file('short.im4p', im4p(body, len(SMALL))), 'the IM4P says', path=short)
        failing = self.fake_lzfse('open(dst, "wb").write(b"half"); sys.exit(1)')
        self.refused(self.file('fail.im4p', im4p(body, len(SMALL))), 'could not decode', path=failing)

    def test_lzfse(self):
        lz = shutil.which('lzfse')
        if not lz:
            if os.environ.get('IM4P_TEST_NEED_LZFSE'):
                self.fail('no lzfse tool')
            self.skipTest('no lzfse tool')
        plain, packed = self.file('kernel', BIG), self.d / 'kernel.lzfse'
        subprocess.run([lz, '-encode', '-i', str(plain), '-o', str(packed)], check=True)
        body = packed.read_bytes()
        self.assertEqual(body[:4], b'bvx2')
        self.unpacks(self.file('lzfse.im4p', im4p(body, len(BIG))), BIG)
        self.unpacks(self.file('lzfse.img4', img4(im4p(body, len(BIG)))), BIG)
        self.refused(self.file('cut.im4p', im4p(body, len(BIG))[:len(body) // 2]), 'cut short')
        self.refused(self.file('early.im4p', im4p(body[:len(body) // 2], len(BIG))), 'damaged?')   # DER whole, stream not

    def serve(self, blob, mode=None):
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Ranges)
        server.blob, server.mode, server.served = blob, mode, 0
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return server, f'http://127.0.0.1:{server.server_address[1]}/test.ipsw'

    def ipsw(self):
        path = self.d / 'big.ipsw'
        with zipfile.ZipFile(path, 'w', zipfile.ZIP_STORED) as z:
            z.writestr('BuildManifest.plist', b'<plist/>')
            z.writestr('disk.dmg', bytes(3 << 20))
            z.writestr('kernelcache.release.vma2', im4p(BIG))
            z.writestr('more.dmg', bytes(1 << 20))
        return path.read_bytes()

    def test_url_reads_only_the_member(self):
        blob = self.ipsw()
        server, url = self.serve(blob)
        self.unpacks(url, BIG)
        self.assertLess(server.served, len(BIG) + (1 << 20), f'{server.served} of {len(blob)} bytes were downloaded')

    def test_url_transfer_that_breaks_off_is_asked_again(self):
        server, url = self.serve(self.ipsw(), 'cut')
        self.unpacks(url, BIG)
        self.assertIsNone(server.mode, 'a transfer was cut')

    def test_url_without_ranges(self):
        _, url = self.serve(self.ipsw(), 'whole')
        self.refused(url, 'byte ranges')


if __name__ == '__main__':
    unittest.main()
