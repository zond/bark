#!/usr/bin/env python3
"""Convert the bark-fw ELF into a UF2 for the T-Echo's UF2 bootloader.

No dependencies beyond Python 3: the ELF program headers are parsed here, so
neither objcopy nor cargo-binutils is needed.

    scripts/uf2.py target/thumbv7em-none-eabihf/release/bark-fw bark-fw.uf2
    scripts/uf2.py --check bark-fw.uf2

Format: UF2 specification (github.com/microsoft/uf2, README "File format"):
512-byte blocks, each carrying 256 bytes of payload for one flash address,
with the family-ID flag set so the bootloader rejects images for other chips.

What goes in: every PT_LOAD segment with file contents, placed at its
physical (load) address. That is the flash image: .vector_table, .text,
.rodata, and the initial values of .data (whose virtual address is in RAM but
whose load address follows .rodata in flash). Gaps between segments are
filled with 0xFF, the erased-flash value (nRF52840 Product Specification,
NVMC chapter), so they are written as if untouched.
"""

import argparse
import struct
import sys

# Application window under the stock bootloader (memory.x, roadmap D6).
APP_START = 0x26000
APP_END = 0xF4000
# Family ID the bootloader accepts for nRF52840 images (its INFO_UF2.TXT
# and the UF2 specification's family list, "nRF52840").
FAMILY_NRF52840 = 0xADA52840

UF2_MAGIC_START0 = 0x0A324655  # "UF2\n"
UF2_MAGIC_START1 = 0x9E5D5157
UF2_MAGIC_END = 0x0AB16F30
UF2_FLAG_FAMILY_ID_PRESENT = 0x00002000
UF2_BLOCK = 512
UF2_PAYLOAD = 256  # bytes of data per block, the size every bootloader accepts
UF2_DATA_FIELD = 476  # bytes reserved for data in each block

PT_LOAD = 1
FLASH_ERASED = 0xFF


def load_segments(elf: bytes) -> list[tuple[int, bytes]]:
    """Return (load address, bytes) for each PT_LOAD segment with file data."""
    if elf[:4] != b"\x7fELF":
        sys.exit("not an ELF file")
    if elf[4] != 1 or elf[5] != 1:
        sys.exit("expected a 32-bit little-endian ELF (thumbv7em)")
    (e_phoff,) = struct.unpack_from("<I", elf, 0x1C)
    e_phentsize, e_phnum = struct.unpack_from("<HH", elf, 0x2A)
    segments = []
    for i in range(e_phnum):
        p_type, p_offset, _vaddr, p_paddr, p_filesz, _memsz, _flags, _align = struct.unpack_from(
            "<8I", elf, e_phoff + i * e_phentsize
        )
        if p_type == PT_LOAD and p_filesz > 0:
            segments.append((p_paddr, elf[p_offset : p_offset + p_filesz]))
    return sorted(segments)


def flash_image(segments: list[tuple[int, bytes]]) -> tuple[int, bytes]:
    """Lay the segments into one contiguous image; check it fits the app window."""
    if not segments:
        sys.exit("ELF has no loadable segments")
    start = segments[0][0]
    end = max(addr + len(data) for addr, data in segments)
    if start != APP_START:
        sys.exit(f"image starts at {start:#x}, expected {APP_START:#x} (check memory.x)")
    if end > APP_END:
        sys.exit(f"image ends at {end:#x}, past the application window end {APP_END:#x}")
    image = bytearray([FLASH_ERASED]) * (end - start)
    previous_end = start
    for addr, data in segments:
        if addr < previous_end:
            sys.exit(f"segments overlap at {addr:#x}")
        image[addr - start : addr - start + len(data)] = data
        previous_end = addr + len(data)
    return start, bytes(image)


def to_uf2(base: int, image: bytes) -> bytes:
    num_blocks = (len(image) + UF2_PAYLOAD - 1) // UF2_PAYLOAD
    out = bytearray()
    for n in range(num_blocks):
        chunk = image[n * UF2_PAYLOAD : (n + 1) * UF2_PAYLOAD]
        chunk = chunk + bytes([FLASH_ERASED]) * (UF2_PAYLOAD - len(chunk))
        header = struct.pack(
            "<8I",
            UF2_MAGIC_START0,
            UF2_MAGIC_START1,
            UF2_FLAG_FAMILY_ID_PRESENT,
            base + n * UF2_PAYLOAD,
            UF2_PAYLOAD,
            n,
            num_blocks,
            FAMILY_NRF52840,
        )
        data = chunk + bytes(UF2_DATA_FIELD - UF2_PAYLOAD)
        out += header + data + struct.pack("<I", UF2_MAGIC_END)
    return bytes(out)


def check(uf2: bytes) -> int:
    """Validate a UF2 file and print a summary. Returns a process exit code."""
    errors = []
    if len(uf2) % UF2_BLOCK:
        errors.append(f"size {len(uf2)} is not a multiple of {UF2_BLOCK}")
    blocks = len(uf2) // UF2_BLOCK
    addresses = []
    for n in range(blocks):
        block = uf2[n * UF2_BLOCK : (n + 1) * UF2_BLOCK]
        m0, m1, flags, addr, size, no, total, family = struct.unpack_from("<8I", block)
        (mend,) = struct.unpack_from("<I", block, UF2_BLOCK - 4)
        if (m0, m1, mend) != (UF2_MAGIC_START0, UF2_MAGIC_START1, UF2_MAGIC_END):
            errors.append(f"block {n}: bad magic")
        if not flags & UF2_FLAG_FAMILY_ID_PRESENT or family != FAMILY_NRF52840:
            errors.append(f"block {n}: family {family:#x} flags {flags:#x}")
        if size != UF2_PAYLOAD:
            errors.append(f"block {n}: payload {size}")
        if no != n or total != blocks:
            errors.append(f"block {n}: numbered {no}/{total}")
        if not (APP_START <= addr and addr + size <= APP_END):
            errors.append(f"block {n}: address {addr:#x} outside application window")
        addresses.append(addr)
    contiguous = all(b - a == UF2_PAYLOAD for a, b in zip(addresses, addresses[1:]))
    print(f"blocks:        {blocks}")
    print(f"first address: {addresses[0]:#x}" if addresses else "first address: -")
    print(f"last address:  {addresses[-1]:#x}" if addresses else "last address:  -")
    print(f"payload bytes: {blocks * UF2_PAYLOAD}")
    print(f"family:        {FAMILY_NRF52840:#x}")
    print(f"contiguous:    {contiguous}")
    for e in errors[:20]:
        print("ERROR:", e)
    print("OK" if not errors else f"{len(errors)} error(s)")
    return 0 if not errors else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="validate an existing UF2 instead")
    ap.add_argument("input", help="ELF to convert, or UF2 with --check")
    ap.add_argument("output", nargs="?", help="UF2 to write")
    args = ap.parse_args()
    with open(args.input, "rb") as f:
        data = f.read()
    if args.check:
        return check(data)
    if not args.output:
        ap.error("output path required")
    base, image = flash_image(load_segments(data))
    uf2 = to_uf2(base, image)
    with open(args.output, "wb") as f:
        f.write(uf2)
    print(f"{args.output}: {len(image)} bytes at {base:#x}, {len(uf2) // UF2_BLOCK} blocks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
