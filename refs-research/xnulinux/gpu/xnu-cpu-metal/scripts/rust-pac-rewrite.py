#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Rewrite plain ARM64 `blr Xn` / `br Xn` (n=0..29) in a linked arm64e Rust Mach-O dylib
into `bl _nmb_pac_call_xN` / `b _nmb_pac_call_xN` trampolines (from RustPacBoundary.c),
skipping the trampolines themselves and RustPacBoundary's own C functions."""
import struct
import subprocess
import sys


def get_symbols(path):
    out = subprocess.check_output(["nm", "-n", path], text=True)
    syms = []
    by_name = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[1] in ("T", "t"):
            addr = int(parts[0], 16)
            name = parts[2]
            syms.append((addr, name))
            by_name[name] = addr
    return syms, by_name


def parse_text_section(data):
    magic, cputype, cpusubtype, filetype, ncmds, sizeofcmds, flags, reserved = struct.unpack_from("<IIIIIIII", data, 0)
    if magic != 0xFEEDFACF:
        raise RuntimeError(f"Expected 64-bit Mach-O, got magic {magic:#x}")
    off = 32
    for _ in range(ncmds):
        cmd, cmdsize = struct.unpack_from("<II", data, off)
        if cmd == 0x19:  # LC_SEGMENT_64
            segname = data[off + 8 : off + 24].split(b"\x00", 1)[0].decode()
            nsects = struct.unpack_from("<I", data, off + 64)[0]
            if segname == "__TEXT":
                sect_off = off + 72
                for _ in range(nsects):
                    sectname = data[sect_off : sect_off + 16].split(b"\x00", 1)[0].decode()
                    addr, size, fileoff = struct.unpack_from("<QQI", data, sect_off + 32)
                    if sectname == "__text":
                        return addr, size, fileoff
                    sect_off += 80
        off += cmdsize
    raise RuntimeError("__TEXT,__text section not found")


def main():
    if len(sys.argv) != 2:
        sys.exit(f"Usage: {sys.argv[0]} <arm64e-mach-o>")
    path = sys.argv[1]
    syms, by_name = get_symbols(path)

    tramp = {}
    for r in range(30):
        name = f"_nmb_pac_call_x{r}"
        if name not in by_name:
            raise RuntimeError(f"Missing trampoline symbol {name}")
        tramp[r] = by_name[name]

    # Build exclusion ranges for RustPacBoundary.c functions (nmb_pac_*, wrap*, signChain, restoreChain)
    skip_prefixes = ("_nmb_pac_", "_wrapCreateInstance", "_wrapCreateMessenger", "_wrapGetInstanceProcAddr", "_signChain", "_restoreChain")
    skip_ranges = []
    for i, (addr, name) in enumerate(syms):
        if name.startswith(skip_prefixes):
            end = syms[i + 1][0] if i + 1 < len(syms) else addr + 4096
            skip_ranges.append((addr, end))

    def is_skipped(pc):
        for s, e in skip_ranges:
            if s <= pc < e:
                return True
        return False

    with open(path, "rb") as f:
        data = bytearray(f.read())

    text_addr, text_size, text_fileoff = parse_text_section(data)
    blr_count = 0
    br_count = 0

    for off in range(0, text_size, 4):
        pc = text_addr + off
        if is_skipped(pc):
            continue
        word = struct.unpack_from("<I", data, text_fileoff + off)[0]
        is_blr = (word & 0xFFFFFC1F) == 0xD63F0000
        is_br = (word & 0xFFFFFC1F) == 0xD61F0000
        if not (is_blr or is_br):
            continue
        reg = (word >> 5) & 31
        if reg >= 30:
            continue
        target = tramp[reg]
        delta = (target - pc) >> 2
        if delta < -(1 << 25) or delta >= (1 << 25):
            raise RuntimeError(f"Branch out of ±128MB range at {pc:#x} -> {target:#x}")
        op = 0x94000000 if is_blr else 0x14000000
        new_word = op | (delta & 0x03FFFFFF)
        struct.pack_into("<I", data, text_fileoff + off, new_word)
        if is_blr:
            blr_count += 1
        else:
            br_count += 1

    with open(path, "wb") as f:
        f.write(data)
    print(f"rust-pac-rewrite: {path}: rewrote {blr_count} blr + {br_count} br (skipped {len(skip_ranges)} PAC boundary fns)")


if __name__ == "__main__":
    main()
