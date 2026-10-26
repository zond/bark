#!/usr/bin/env python3
"""Pack bark-fw, and the loader with it, into a UF2 for the T-Echo's UF2 bootloader.

No dependencies beyond Python 3: the ELF headers and symbols are parsed
here, so neither objcopy nor cargo-binutils is needed.

    # a board that already has the loader: the firmware alone
    scripts/uf2.py target/thumbv7em-none-eabihf/release/bark-fw bark-fw.uf2
    # a board without it, or to replace it: the loader and the firmware
    scripts/uf2.py --loader ../bark-loader/target/thumbv7em-none-eabihf/release/bark-loader \\
        target/thumbv7em-none-eabihf/release/bark-fw bark.uf2
    scripts/uf2.py --check bark.uf2

Format: UF2 specification (github.com/microsoft/uf2, README "File format"):
512-byte blocks, each carrying 256 bytes of payload for one flash address,
with the family-ID flag set so the bootloader rejects images for other chips.
The blocks need not be contiguous.

What goes in: every PT_LOAD segment with file contents, placed at its
physical (load) address. That is the flash image: .vector_table, .text,
.rodata, and the initial values of .data (whose virtual address is in RAM but
whose load address follows .rodata in flash). Gaps between segments are
filled with 0xFF, the erased-flash value (nRF52840 Product Specification,
NVMC chapter), so they are written as if untouched.

The regions come from the symbols the linker scripts define, which
bark-image/src/layout.rs writes. Every UF2 also writes the loader's state
page and the staged image's record as erased, so that a copied firmware
always boots as it is: no swap asked for before the copy takes place over it.
"""

from __future__ import annotations

import argparse
import struct
import sys

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
SHT_SYMTAB = 2
FLASH_ERASED = 0xFF

# What the board's bootloader lets a UF2 write: from where it jumps to
# below itself (bark-image/src/layout.rs, APPLICATION_START and
# BOOTLOADER_START).
WINDOW_START = 0x26000
WINDOW_END = 0xF4000


def check_elf(elf: bytes) -> None:
    if elf[:4] != b"\x7fELF":
        sys.exit("not an ELF file")
    if elf[4] != 1 or elf[5] != 1:
        sys.exit("expected a 32-bit little-endian ELF (thumbv7em)")


def load_segments(elf: bytes) -> list[tuple[int, bytes]]:
    """Return (load address, bytes) for each PT_LOAD segment with file data."""
    check_elf(elf)
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


def symbols(elf: bytes) -> dict[str, int]:
    """Every symbol's name and value, from the ELF's symbol table."""
    check_elf(elf)
    (e_shoff,) = struct.unpack_from("<I", elf, 0x20)
    e_shentsize, e_shnum = struct.unpack_from("<HH", elf, 0x2E)
    sections = [struct.unpack_from("<10I", elf, e_shoff + i * e_shentsize) for i in range(e_shnum)]
    found = {}
    for sh in sections:
        _name, sh_type, _flags, _addr, offset, size, link, _info, _align, entsize = sh
        if sh_type != SHT_SYMTAB:
            continue
        strtab = sections[link]
        str_off = strtab[4]
        for at in range(offset, offset + size, entsize):
            st_name, st_value = struct.unpack_from("<II", elf, at)
            end = elf.index(b"\0", str_off + st_name)
            found[elf[str_off + st_name : end].decode()] = st_value
    return found


def region(syms: dict[str, int], name: str) -> tuple[int, int]:
    try:
        return syms[f"{name}_start"], syms[f"{name}_end"]
    except KeyError:
        sys.exit(f"the ELF has no {name}_start/_end: built before bark-image wrote its memory.x?")


def flash_image(segments: list[tuple[int, bytes]], start: int, end: int, what: str) -> bytes:
    """Lay the segments into one contiguous image; check it fills its region from the start."""
    if not segments:
        sys.exit(f"{what}: the ELF has no loadable segments")
    first = segments[0][0]
    last = max(addr + len(data) for addr, data in segments)
    if first != start:
        sys.exit(f"{what} starts at {first:#x}, expected {start:#x}")
    if last > end:
        sys.exit(f"{what} ends at {last:#x}, past its region's end {end:#x}")
    image = bytearray([FLASH_ERASED]) * (last - first)
    previous_end = first
    for addr, data in segments:
        if addr < previous_end:
            sys.exit(f"{what}: segments overlap at {addr:#x}")
        image[addr - first : addr - first + len(data)] = data
        previous_end = addr + len(data)
    return bytes(image)


def blocks(base: int, image: bytes) -> list[tuple[int, bytes]]:
    """(address, 256 bytes) for each UF2 payload of `image` at `base`."""
    out = []
    for at in range(0, len(image), UF2_PAYLOAD):
        chunk = image[at : at + UF2_PAYLOAD]
        out.append((base + at, chunk + bytes([FLASH_ERASED]) * (UF2_PAYLOAD - len(chunk))))
    return out


def to_uf2(payloads: list[tuple[int, bytes]]) -> bytes:
    out = bytearray()
    total = len(payloads)
    for n, (addr, chunk) in enumerate(payloads):
        header = struct.pack(
            "<8I",
            UF2_MAGIC_START0,
            UF2_MAGIC_START1,
            UF2_FLAG_FAMILY_ID_PRESENT,
            addr,
            UF2_PAYLOAD,
            n,
            total,
            FAMILY_NRF52840,
        )
        out += header + chunk + bytes(UF2_DATA_FIELD - UF2_PAYLOAD) + struct.pack("<I", UF2_MAGIC_END)
    return bytes(out)


def pack(firmware_elf: bytes, loader_elf: bytes | None) -> tuple[bytes, list[str]]:
    """The UF2, and a line for each region it writes."""
    syms = symbols(firmware_elf)
    payloads = []
    lines = []
    if loader_elf is not None:
        start, end = region(syms, "__bark_loader")
        if region(symbols(loader_elf), "__bark_loader") != (start, end):
            sys.exit("the loader and the firmware were built from different layouts")
        loader = flash_image(load_segments(loader_elf), start, end, "loader")
        payloads += blocks(start, loader)
        lines.append(f"loader:   {len(loader)} bytes at {start:#x}")
    for name, what in (("__bootloader_state", "state"), ("__bark_record", "record")):
        start, end = region(syms, name)
        payloads += blocks(start, bytes([FLASH_ERASED]) * (end - start))
        lines.append(f"{what + ':':<9} {end - start} bytes at {start:#x}, erased")
    start, end = region(syms, "__bootloader_active")
    firmware = flash_image(load_segments(firmware_elf), start, end, "firmware")
    payloads += blocks(start, firmware)
    lines.append(f"firmware: {len(firmware)} bytes at {start:#x}")
    return to_uf2(sorted(payloads)), lines


def check(uf2: bytes) -> int:
    """Validate a UF2 file and print a summary. Returns a process exit code."""
    errors = []
    if len(uf2) % UF2_BLOCK:
        errors.append(f"size {len(uf2)} is not a multiple of {UF2_BLOCK}")
    count = len(uf2) // UF2_BLOCK
    addresses = []
    for n in range(count):
        block = uf2[n * UF2_BLOCK : (n + 1) * UF2_BLOCK]
        m0, m1, flags, addr, size, no, total, family = struct.unpack_from("<8I", block)
        (mend,) = struct.unpack_from("<I", block, UF2_BLOCK - 4)
        if (m0, m1, mend) != (UF2_MAGIC_START0, UF2_MAGIC_START1, UF2_MAGIC_END):
            errors.append(f"block {n}: bad magic")
        if not flags & UF2_FLAG_FAMILY_ID_PRESENT or family != FAMILY_NRF52840:
            errors.append(f"block {n}: family {family:#x} flags {flags:#x}")
        if size != UF2_PAYLOAD:
            errors.append(f"block {n}: payload {size}")
        if no != n or total != count:
            errors.append(f"block {n}: numbered {no}/{total}")
        if not (WINDOW_START <= addr and addr + size <= WINDOW_END):
            errors.append(f"block {n}: address {addr:#x} outside {WINDOW_START:#x}..{WINDOW_END:#x}")
        if addresses and addr <= addresses[-1]:
            errors.append(f"block {n}: address {addr:#x} not above the one before")
        addresses.append(addr)
    runs = []
    for addr in addresses:
        if runs and addr == runs[-1][1]:
            runs[-1][1] = addr + UF2_PAYLOAD
        else:
            runs.append([addr, addr + UF2_PAYLOAD])
    print(f"blocks:        {count}")
    print(f"payload bytes: {count * UF2_PAYLOAD}")
    print(f"family:        {FAMILY_NRF52840:#x}")
    for start, end in runs:
        print(f"range:         {start:#x}..{end:#x} ({end - start} bytes)")
    for e in errors[:20]:
        print("ERROR:", e)
    print("OK" if not errors else f"{len(errors)} error(s)")
    return 0 if not errors else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="validate an existing UF2 instead")
    ap.add_argument("--loader", help="the loader's ELF, to put in front of the firmware")
    ap.add_argument("input", help="the firmware's ELF, or a UF2 with --check")
    ap.add_argument("output", nargs="?", help="UF2 to write")
    args = ap.parse_args()
    with open(args.input, "rb") as f:
        data = f.read()
    if args.check:
        return check(data)
    if not args.output:
        ap.error("output path required")
    loader = None
    if args.loader:
        with open(args.loader, "rb") as f:
            loader = f.read()
    uf2, lines = pack(data, loader)
    with open(args.output, "wb") as f:
        f.write(uf2)
    for line in lines:
        print(line)
    print(f"{args.output}: {len(uf2) // UF2_BLOCK} blocks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
