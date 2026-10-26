#!/usr/bin/env python3
"""Pack bark-fw into an image to stage, and stage it over the serial console.

A bench tool until the update has a transport: the console build takes
the image in chunks (`stage`, `chunk`, `commit`, then `reboot`) and the
loader swaps it in at the next boot (bark-fw/README.md, "Updates").

    BARK_IMAGE_VERSION=2 cargo build --release
    scripts/image.py pack target/thumbv7em-none-eabihf/release/bark-fw bark-fw.img
    scripts/image.py stage bark-fw.img /dev/ttyACM0 [--reboot]

An image is the 256-byte header of bark-image/src/lib.rs (magic "bark",
format 1, version, length, SHA-256 of the image, room for signatures)
followed by the flash image from the firmware's slot, as scripts/uf2.py
lays it. The version is the firmware's own, read from its
BARK_IMAGE_VERSION symbol, so that the header cannot say another.
Standard library only; run with python3 -I.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import select
import struct
import sys
import termios
import time
import tty

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import uf2  # noqa: E402  (scripts/uf2.py, next to this file)

HEADER_LEN = 256
MAGIC = b"bark"
FORMAT = 1
# Bytes the console's `chunk` takes at once (bark_image::boot::CHUNK).
CHUNK = 256
# How long to wait for the console's answer to one command. A chunk that
# enters a new page waits for its erase, 85 ms (nRF52840 Product
# Specification, NVMC, tERASEPAGE); `commit` hashes up to 352 KiB.
ANSWER_S = 10.0


def header(version: int, image: bytes) -> bytes:
    h = MAGIC + bytes([FORMAT, 0, 0, 0]) + struct.pack("<II", version, len(image))
    h += hashlib.sha256(image).digest()
    return h + bytes(HEADER_LEN - len(h))


def pack(elf: bytes) -> tuple[bytes, int]:
    """The image file and its version."""
    syms = uf2.symbols(elf)
    start, end = uf2.region(syms, "__bootloader_active")
    image = uf2.flash_image(uf2.load_segments(elf), start, end, "firmware")
    at = syms.get("BARK_IMAGE_VERSION")
    if at is None or not start <= at < start + len(image) - 4:
        sys.exit("the ELF has no BARK_IMAGE_VERSION in its image")
    (version,) = struct.unpack_from("<I", image, at - start)
    return header(version, image) + image, version


def answer(fd: int, buf: bytearray, words: tuple[str, ...]) -> str:
    """The first line from the console that holds one of `words`."""
    deadline = time.monotonic() + ANSWER_S
    while True:
        while b"\n" in buf:
            line, _, rest = bytes(buf).partition(b"\n")
            buf[:] = rest
            text = line.decode(errors="replace").strip()
            if not text.startswith(">") and any(w in text for w in words):
                return text
        left = deadline - time.monotonic()
        if left <= 0 or not select.select([fd], [], [], left)[0]:
            sys.exit(f"no answer from the console within {ANSWER_S} s")
        buf += os.read(fd, 4096)


def stage(path: str, port: str, reboot: bool) -> int:
    with open(path, "rb") as f:
        data = f.read()
    head, image = data[:HEADER_LEN], data[HEADER_LEN:]
    if head[:4] != MAGIC or len(image) != struct.unpack_from("<I", head, 12)[0]:
        sys.exit(f"{path} is not an image from `image.py pack`")
    fd = os.open(port, os.O_RDWR | os.O_NOCTTY)
    tty.setraw(fd)
    termios.tcflush(fd, termios.TCIOFLUSH)
    buf = bytearray()

    def send(line: str, words: tuple[str, ...]) -> str:
        os.write(fd, (line + "\r\n").encode())
        got = answer(fd, buf, words)
        if "refused" in got:
            sys.exit(got)
        return got

    print(send("stage " + head.hex(), ("stage", "refused")))
    for at in range(0, len(image), CHUNK):
        send(f"chunk {at} {image[at:at + CHUNK].hex()}", ("chunk",))
        print(f"\r{at + min(CHUNK, len(image) - at)} / {len(image)} bytes", end="", flush=True)
    print()
    print(send("commit", ("commit",)))
    if reboot:
        os.write(fd, b"reboot\r\n")
        print("reboot sent")
    os.close(fd)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="command", required=True)
    p = sub.add_parser("pack", help="write the image file from the firmware's ELF")
    p.add_argument("elf")
    p.add_argument("output")
    s = sub.add_parser("stage", help="send an image file to a console build")
    s.add_argument("image")
    s.add_argument("port", help="the serial port, /dev/ttyACM0 or the like")
    s.add_argument("--reboot", action="store_true", help="reboot once it is committed")
    args = ap.parse_args()
    if args.command == "pack":
        with open(args.elf, "rb") as f:
            out, version = pack(f.read())
        with open(args.output, "wb") as f:
            f.write(out)
        print(f"{args.output}: version {version}, {len(out) - HEADER_LEN} bytes of image")
        return 0
    return stage(args.image, args.port, args.reboot)


if __name__ == "__main__":
    sys.exit(main())
