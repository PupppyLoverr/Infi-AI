#!/usr/bin/env python3
"""Render the welcome Markdown to a real A4 PDF with only the standard library.

Usage: mkpdf.py IN.md OUT.pdf

Handles the subset the guide uses: # / ## headings, paragraphs, - bullets,
pipe tables and **bold**/`code` markers. Text is set in the PDF base-14
fonts (Helvetica, Helvetica-Bold), so nothing is embedded.
"""
import re
import sys
import zlib

# Helvetica advance widths (1/1000 em) for ASCII 32..126, from the AFM.
HELV = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
]
# Non-ASCII characters the guide may use, mapped to WinAnsiEncoding bytes.
WINANSI = {"\u2014": 0x97, "\u2013": 0x96, "\u2022": 0x95, "\u2019": 0x92,
           "\u201c": 0x93, "\u201d": 0x94, "\u00d7": 0xD7, "\u00b7": 0xB7}

PAGE_W, PAGE_H, MARGIN = 595, 842, 64
TEXT_W = PAGE_W - 2 * MARGIN
INK, MUTED, RULE = "0.11 0.10 0.14", "0.38 0.37 0.42", "0.85 0.84 0.88"


def width(text, size, bold=False):
    w = sum(HELV[ord(c) - 32] if 32 <= ord(c) < 127 else 556 for c in text)
    return w * size / 1000 * (1.06 if bold else 1.0)


def encode(text):
    out = bytearray()
    for c in text:
        b = WINANSI.get(c, ord(c) if ord(c) < 256 else ord("?"))
        if b in (0x28, 0x29, 0x5C):
            out += b"\\"
        out.append(b)
    return bytes(out)


def clean(text):
    return re.sub(r"\*\*|`", "", text)


def wrap(text, size, avail, bold=False):
    lines, cur = [], ""
    for word in text.split():
        cand = f"{cur} {word}".strip()
        if cur and width(cand, size, bold) > avail:
            lines.append(cur)
            cur = word
        else:
            cur = cand
    return lines + [cur] if cur else lines


class Doc:
    def __init__(self, title):
        self.title, self.pages, self.ops, self.y = title, [], [], 0
        self.new_page()

    def new_page(self):
        if self.ops:
            self.pages.append(self.ops)
        self.ops, self.y = [], PAGE_H - MARGIN

    def need(self, h):
        if self.y - h < MARGIN + 24:
            self.new_page()

    def text(self, x, s, size, bold=False, colour=INK):
        font = "F2" if bold else "F1"
        self.ops.append(b"BT %s rg /%s %g Tf %g %g Td (" % (
            colour.encode(), font.encode(), size, x, self.y) + encode(s) + b") Tj ET")

    def rule(self, y, x0=MARGIN, x1=PAGE_W - MARGIN):
        self.ops.append(b"%s RG 0.6 w %g %g m %g %g l S" % (RULE.encode(), x0, y, x1, y))

    def heading(self, s, size):
        self.need(size * 2.4)
        self.y -= size * (1.5 if size < 20 else 0.4)
        self.text(MARGIN, s, size, bold=True)
        self.y -= size * 0.8

    def para(self, s, indent=0, bullet=False, size=10.5):
        lead = size * 1.45
        lines = wrap(s, size, TEXT_W - indent)
        self.need(lead * min(len(lines), 2))
        for i, line in enumerate(lines):
            self.need(lead)
            self.y -= lead
            if bullet and i == 0:
                self.text(MARGIN + indent - 11, "\u2022", size, colour=MUTED)
            self.text(MARGIN + indent, line, size)
        self.y -= size * 0.5

    def table(self, rows, size=10):
        lead, col = size * 1.9, TEXT_W * 0.42
        self.y -= size * 0.4
        for r, cells in enumerate(rows):
            self.need(lead)
            self.y -= lead
            self.text(MARGIN + 4, cells[0], size, bold=(r == 0))
            self.text(MARGIN + col, cells[1], size, bold=(r == 0),
                      colour=INK if r == 0 else MUTED)
            self.rule(self.y - size * 0.7)
        self.y -= size

    def finish(self):
        self.pages.append(self.ops)
        n = len(self.pages)
        objs = [None,
                b"<< /Type /Catalog /Pages 2 0 R >>",
                None,
                b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
                b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
                b"<< /Title (" + encode(self.title) + b") /Producer (CosmosOS image build) >>"]
        kids = []
        for i, ops in enumerate(self.pages, 1):
            self.y = MARGIN - 4
            self.ops = ops
            footer = f"{self.title}  \u00b7  {i} of {n}"
            self.text(PAGE_W - MARGIN - width(footer, 8.5), footer, 8.5, colour=MUTED)
            body = zlib.compress(b"\n".join(ops))
            objs.append(b"<< /Length %d /Filter /FlateDecode >>\nstream\n" % len(body)
                        + body + b"\nendstream")
            content = len(objs) - 1
            objs.append(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] "
                        b"/Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents %d 0 R >>"
                        % (PAGE_W, PAGE_H, content))
            kids.append(len(objs) - 1)
        objs[2] = b"<< /Type /Pages /Count %d /Kids [%s] >>" % (
            n, b" ".join(b"%d 0 R" % k for k in kids))
        out, offsets = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"), []
        for i, o in enumerate(objs[1:], 1):
            offsets.append(len(out))
            out += b"%d 0 obj\n" % i + o + b"\nendobj\n"
        xref = len(out)
        out += b"xref\n0 %d\n0000000000 65535 f \n" % len(objs)
        out += b"".join(b"%010d 00000 n \n" % off for off in offsets)
        out += b"trailer\n<< /Size %d /Root 1 0 R /Info 5 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
            len(objs), xref)
        return bytes(out)


def render(md, title):
    doc, para, table = Doc(title), [], []

    def flush():
        if para:
            doc.para(clean(" ".join(para)))
            para.clear()
        if table:
            doc.table(table)
            table.clear()

    for raw in md.splitlines():
        line = raw.strip()
        if not line:
            flush()
        elif line.startswith("|"):
            cells = [clean(c.strip()) for c in line.strip("|").split("|")]
            if not all(set(c) <= set("-: ") for c in cells):
                table.append(cells)
        elif line.startswith("## "):
            flush()
            doc.heading(clean(line[3:]), 15)
        elif line.startswith("# "):
            flush()
            doc.heading(clean(line[2:]), 26)
            doc.rule(doc.y)
            doc.y -= 6
        elif line.startswith("- "):
            flush()
            doc.para(clean(line[2:]), indent=14, bullet=True)
        else:
            para.append(line)
    flush()
    return doc.finish()


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    with open(src, encoding="utf-8") as f:
        pdf = render(f.read(), "CosmosOS User Guide")
    with open(dst, "wb") as f:
        f.write(pdf)
