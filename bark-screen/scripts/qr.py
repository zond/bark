#!/usr/bin/env python3
"""The QR code a node's screen shows (bark-screen/src/qr.rs, roadmap D160).

    qr.py generate   write bark-screen/src/qr/symbol.rs from the address
    qr.py check      draw the screen's picture on the host and decode it

`generate` encodes the project's address with the qrcodegen package and
writes the modules as a Rust constant; the host test
`the_symbol_is_the_address` encodes it again with qrcodegen's Rust crate
and compares, so the constant cannot drift from the address. `check` has
the host test write the picture as the screen draws it, emblem and threads included, then
decodes it with ZXing-C++ and with OpenCV at the screen's size and
scaled, and writes it as a PNG beside the PBM.

Needs qrcodegen, zxing-cpp, opencv-python-headless and pillow; a venv keeps
them off the system Python:

    python3 -m venv .venv
    .venv/bin/pip install qrcodegen zxing-cpp opencv-python-headless pillow
    .venv/bin/python bark-screen/scripts/qr.py check
"""

import os
import subprocess
import sys
from pathlib import Path

CRATE = Path(__file__).resolve().parent.parent
SYMBOL = CRATE / "src" / "qr" / "symbol.rs"
PICTURE = CRATE.parent / "target" / "bark-screen" / "code.pbm"

# What the code says: the project's page without its scheme, which a
# phone's camera adds. Lowercase, so byte mode: the alphanumeric mode has
# capitals only, and the page's path is case-sensitive.
ADDRESS = "zond.github.io/bark"
# Level H restores about 30 % of the codewords (ISO/IEC 18004, error
# correction levels),
# which is what lets the emblem cover a few modules.
LEVEL = "h"


def generate():
    from qrcodegen import QrCode, QrSegment

    segs = [QrSegment.make_bytes(ADDRESS.encode())]
    q = QrCode.encode_segments(segs, QrCode.Ecc.HIGH, boostecl=False)
    size = q.get_size()
    rows = [[q.get_module(x, y) for x in range(size)] for y in range(size)]
    lines = [
        "// Written by bark-screen/scripts/qr.py generate; do not edit.",
        "",
        f'/// What the code says.\npub const ADDRESS: &str = "{ADDRESS}";',
        f"/// The symbol's version, the smallest that holds the address at level H.\npub const VERSION: u8 = {q.get_version()};",
        f"/// The data mask chosen by the standard's penalty rules.\npub const MASK: u8 = {q.get_mask()};",
        f"/// Modules a side: 17 + 4 × version (ISO/IEC 18004, symbol versions).\npub const SIZE: usize = {size};",
        "/// The modules, a row each from the top, the leftmost in bit 0, 1 dark.",
        f"pub const MODULES: [u32; SIZE] = [",
    ]
    for r in rows:
        bits = sum(1 << i for i, m in enumerate(r) if m)
        lines.append(f"    0x{bits:08x},")
    lines.append("];")
    SYMBOL.parent.mkdir(exist_ok=True)
    SYMBOL.write_text("\n".join(lines) + "\n")
    print(f"{SYMBOL}: version {q.get_version()}-H, mask {q.get_mask()}, {size} × {size}")


def read_pbm(path):
    """A plain or raw PBM as rows of 0 (white) and 255 (black)."""
    data = path.read_bytes()
    assert data[:2] == b"P4", "raw PBM expected"
    fields = data.split(maxsplit=3)
    w, h = int(fields[1]), int(fields[2])
    body = fields[3]
    stride = (w + 7) // 8
    return [
        [0 if body[y * stride + x // 8] & (0x80 >> (x % 8)) else 255 for x in range(w)]
        for y in range(h)
    ]


def check():
    import cv2
    import numpy as np
    import zxingcpp
    from PIL import Image

    env = dict(os.environ, BARK_SCREEN_PICTURE=str(PICTURE))
    PICTURE.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["cargo", "test", "-q", "-p", "bark-screen", "--lib", "the_code_reads"],
        cwd=CRATE.parent,
        env=env,
        check=True,
    )
    gray = np.array(read_pbm(PICTURE), dtype=np.uint8)
    png = PICTURE.with_suffix(".png")
    Image.fromarray(gray).resize((600, 600), Image.NEAREST).save(png)
    print(f"{png}")
    ok = True
    for scale in [1, 2, 3, 0.75]:
        img = cv2.resize(gray, None, fx=scale, fy=scale, interpolation=cv2.INTER_AREA)
        z = zxingcpp.read_barcodes(img)
        zt = [r.text for r in z if r.format == zxingcpp.BarcodeFormat.QRCode]
        o, _, _ = cv2.QRCodeDetector().detectAndDecode(img)
        good = zt == [ADDRESS] and o == ADDRESS
        ok &= good
        print(f"scale {scale}: zxing-cpp {zt}, opencv {o!r}: {'ok' if good else 'FAILED'}")
    if not ok:
        sys.exit(1)


if __name__ == "__main__":
    {"generate": generate, "check": check}[sys.argv[1]]()
