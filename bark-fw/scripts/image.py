#!/usr/bin/env python3
"""Pack bark-fw into an image to stage, sign it, and stage it over the serial console.

The image is what the phone link stages too, and what a release carries
as bark-fw-<N>.img; the console build takes it in chunks (`stage`,
`chunk`, `commit`, then `reboot`), and the loader swaps it in at the
next boot (bark-fw/README.md, "Updates").

    scripts/image.py keygen signing.pem
    BARK_IMAGE_VERSION=2 cargo build --release
    scripts/image.py pack target/thumbv7em-none-eabihf/release/bark-fw bark-fw.img
    scripts/image.py sign bark-fw.img signing.pem --key-id 1   # or --elf ELF
    scripts/image.py check bark-fw.img running-bark-fw.elf
    scripts/image.py stage bark-fw.img /dev/ttyACM0 [--running ELF] [--reboot]

An image is the 256-byte header of bark-image/src/lib.rs (magic "bark",
format 1, version, length, SHA-256 of the image, three signature slots)
followed by the flash image from the firmware's slot, as scripts/uf2.py
lays it. The version is the firmware's own, read from its
BARK_IMAGE_VERSION symbol, so that the header cannot say another. A
signature is Ed25519 (RFC 8032) over the header's first 48 bytes, by a
key the running firmware holds under the byte that names it
(bark-image/src/sign.rs).

pack and stage need the standard library only; keygen, sign and check
need the `cryptography` package. Run with python3 -I.
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
# The signatures (bark-image/src/sign.rs): the header's bytes they cover,
# then three slots of a byte naming the key and a signature of 64 bytes
# (RFC 8032, 5.1.6).
SIGNED_LEN = 48
SLOTS = 3
SIGNATURE_LEN = 64
SLOT_LEN = 1 + SIGNATURE_LEN
# The bench key pair, for open builds alone (bark-fw/README.md, "Updates").
KEYS = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "keys")
BENCH_KEY = os.path.join(KEYS, "bench.pem")


def bench_key_id() -> int:
    """The byte naming the bench key, from keys/bench.pub (id:hex)."""
    with open(os.path.join(KEYS, "bench.pub")) as f:
        return int(f.read().split(":")[0])


# The most blocks an image the mesh carries has, the table's among them, and
# a block's bytes: 64 pieces of 247 (bark-image/src/pieces.rs, MAX_BLOCKS;
# docs/protocol.md, "Updates over the mesh"). A packed image longer than
# 47 blocks cannot travel over the mesh.
MESH_BLOCKS = 48
BLOCK_BYTES = 64 * 247

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
    uf2.check_keys(image, start, syms)
    at = syms.get("BARK_IMAGE_VERSION")
    if at is None or not start <= at < start + len(image) - 4:
        sys.exit("the ELF has no BARK_IMAGE_VERSION in its image")
    (version,) = struct.unpack_from("<I", image, at - start)
    if version == 0:
        # Staged only above the running version, 0 never is.
        sys.exit("the ELF was built without BARK_IMAGE_VERSION: version 0 cannot be staged")
    packed = header(version, image) + image
    blocks = 1 + -(-len(packed) // BLOCK_BYTES)
    if blocks > MESH_BLOCKS:
        sys.exit(
            f"the image packs to {len(packed)} bytes, {blocks} blocks with the table: "
            f"more than the {MESH_BLOCKS} the mesh carries, {(MESH_BLOCKS - 1) * BLOCK_BYTES} bytes "
            "(bark-image/src/pieces.rs, MAX_BLOCKS)"
        )
    return packed, version


def ed25519():
    """The `cryptography` package's Ed25519, imported only where needed so
    that pack and stage run on the standard library alone."""
    try:
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric import ed25519 as ed
    except ImportError:
        sys.exit("keygen, sign and check need the `cryptography` package (apt install python3-cryptography)")
    return ed, serialization


def read_image(path: str) -> tuple[bytes, bytes]:
    """The header and the image of an image file, its hash checked."""
    with open(path, "rb") as f:
        data = f.read()
    head, image = data[:HEADER_LEN], data[HEADER_LEN:]
    if len(head) != HEADER_LEN or head[:4] != MAGIC or len(image) != struct.unpack_from("<I", head, 12)[0]:
        sys.exit(f"{path} is not an image from `image.py pack`")
    if hashlib.sha256(image).digest() != head[16:48]:
        sys.exit(f"{path}: the image does not match its header's SHA-256")
    return head, image


def slots(head: bytes) -> list[tuple[int, bytes]]:
    """(key id, signature) of each slot, 0 for an empty one."""
    at = [SIGNED_LEN + i * SLOT_LEN for i in range(SLOTS)]
    return [(head[a], head[a + 1 : a + SLOT_LEN]) for a in at]


def keygen(path: str) -> int:
    ed, serialization = ed25519()
    key = ed.Ed25519PrivateKey.generate()
    pem = key.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()
    )
    # Readable by its owner alone, and never over a key already there.
    try:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        sys.exit(f"{path} exists: a key is never written over")
    with os.fdopen(fd, "wb") as f:
        f.write(pem)
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    print(f"{path}: private key; its public key:")
    print(public.hex())
    return 0


def elf_keys(elf_path: str) -> tuple[dict[int, bytes], int]:
    """The signing keys, by id, and the version of the firmware in an ELF."""
    with open(elf_path, "rb") as f:
        elf = f.read()
    syms = uf2.symbols(elf)
    start, end = uf2.region(syms, "__bootloader_active")
    firmware = uf2.flash_image(uf2.load_segments(elf), start, end, "firmware")
    keys = dict(uf2.signing_keys(firmware, start, syms))
    if not keys:
        sys.exit(f"{elf_path} holds no signing keys")
    return keys, uf2.symbol_word(firmware, start, syms, "BARK_IMAGE_VERSION")


def sign(path: str, key_path: str, key_id: int | None, elf_path: str | None) -> int:
    ed, serialization = ed25519()
    head, image = read_image(path)
    with open(key_path, "rb") as f:
        key = serialization.load_pem_private_key(f.read(), password=None)
    if not isinstance(key, ed.Ed25519PrivateKey):
        sys.exit(f"{key_path} is not an Ed25519 private key")
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    if elf_path is not None:
        ids = [i for i, k in elf_keys(elf_path)[0].items() if k == public]
        if not ids:
            sys.exit(f"{elf_path} does not hold the key {public.hex()} of {key_path}")
        key_id = ids[0]
    if key_id is None or not 1 <= key_id <= 255:
        sys.exit("a key id is 1 to 255")
    taken = slots(head)
    if any(i == key_id for i, _ in taken):
        sys.exit(f"{path} is already signed by key {key_id}")
    free = [n for n, (i, _) in enumerate(taken) if i == 0]
    if not free:
        sys.exit(f"{path} has no free signature slot")
    at = SIGNED_LEN + free[0] * SLOT_LEN
    signature = key.sign(head[:SIGNED_LEN])
    head = head[:at] + bytes([key_id]) + signature + head[at + SLOT_LEN :]
    with open(path, "wb") as f:
        f.write(head + image)
    print(f"{path}: signed in slot {free[0]} by key {key_id}, {public.hex()}")
    return 0


def accepted_by(path: str, elf_path: str) -> str:
    """Why the firmware in `elf_path` would stage the image at `path`;
    exits with why not. The firmware's own check is the rule
    (bark-image/src/sign.rs, verify); this one spares a trip."""
    ed, _ = ed25519()
    from cryptography.exceptions import InvalidSignature

    head, _ = read_image(path)
    keys, running = elf_keys(elf_path)
    (version,) = struct.unpack_from("<I", head, 8)
    if version <= running:
        sys.exit(f"{path} is version {version}, not above the firmware's {running}")
    signed = head[:SIGNED_LEN]
    for key_id, signature in slots(head):
        if key_id in keys:
            try:
                ed.Ed25519PublicKey.from_public_bytes(keys[key_id]).verify(signature, signed)
                return f"{path}: version {version} over {running}, signed by key {key_id}, which {elf_path} holds"
            except InvalidSignature:
                pass
    held = ", ".join(str(k) for k in sorted(keys))
    found = ", ".join(str(i) for i, _ in slots(head) if i) or "none"
    sys.exit(f"{path}: no signature verifies under {elf_path}'s keys {held} (signed by: {found})")


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


def stage(path: str, port: str, reboot: bool, running: str | None = None, after: tuple[str, ...] = ()) -> int:
    """Send the image, commit it, then each console command of `after`,
    each answered by a line that holds it whole, and reboot if asked."""
    if running is not None:
        print(accepted_by(path, running))
    head, image = read_image(path)
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

    # The console's line may hold bytes the host's tty echoed back while a
    # port was opened before raw mode took hold (README, "Log"). A bare
    # line end closes that line, which the console answers with a warning,
    # and `status`, answered after it, marks where its answers end: what
    # came before is dropped, so `stage` starts a clean line and its answer
    # is the next one read.
    os.write(fd, b"\r\n")
    send("status", ("status id=",))
    print(send("stage " + head.hex(), ("stage", "refused")))
    for at in range(0, len(image), CHUNK):
        send(f"chunk {at} {image[at:at + CHUNK].hex()}", ("chunk",))
        print(f"\r{at + min(CHUNK, len(image) - at)} / {len(image)} bytes", end="", flush=True)
    print()
    print(send("commit", ("commit",)))
    for line in after:
        print(send(line, (line,)))
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
    k = sub.add_parser("keygen", help="write a new Ed25519 private key and print its public key")
    k.add_argument("key", help="the private key file to write, PEM")
    g = sub.add_parser("sign", help="add a signature to an image file, in its next free slot")
    g.add_argument("image")
    g.add_argument("key", help="the private key file, from keygen")
    which = g.add_mutually_exclusive_group(required=True)
    which.add_argument("--key-id", type=int, help="the byte naming the key, 1 to 255")
    which.add_argument("--elf", help="name the key as this firmware does, and refuse a key it does not hold")
    c = sub.add_parser("check", help="whether a firmware would stage an image file")
    c.add_argument("image")
    c.add_argument("elf", help="the ELF of the firmware the board runs")
    s = sub.add_parser("stage", help="send an image file to a console build")
    s.add_argument("image")
    s.add_argument("port", help="the serial port, /dev/ttyACM0 or the like")
    s.add_argument("--running", metavar="ELF", help="check first against the ELF of the firmware the board runs")
    s.add_argument("--reboot", action="store_true", help="reboot once it is committed")
    args = ap.parse_args()
    if args.command == "keygen":
        return keygen(args.key)
    if args.command == "sign":
        return sign(args.image, args.key, args.key_id, args.elf)
    if args.command == "check":
        print(accepted_by(args.image, args.elf))
        return 0
    if args.command == "pack":
        with open(args.elf, "rb") as f:
            out, version = pack(f.read())
        with open(args.output, "wb") as f:
            f.write(out)
        print(f"{args.output}: version {version}, {len(out) - HEADER_LEN} bytes of image")
        return 0
    return stage(args.image, args.port, args.reboot, args.running)


if __name__ == "__main__":
    sys.exit(main())
