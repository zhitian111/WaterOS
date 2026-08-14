#!/usr/bin/env python3
"""Minimal legacy U-Boot/PMON uImage writer.

Some mkimage builds lack LoongArch in their architecture table; this writes the
same 64-byte legacy image header + payload without depending on u-boot-tools.
"""

from __future__ import annotations

import argparse
import struct
import zlib
from pathlib import Path

IH_MAGIC = 0x27051956
IH_OS_LINUX = 5
IH_ARCH_LOONGARCH = 24
IH_TYPE_KERNEL = 2
IH_COMP_NONE = 0
HEADER_SIZE = 64


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--load", required=True, help="load address (hex)")
    parser.add_argument("--entry", required=True, help="entry address (hex)")
    parser.add_argument("--name", default="WaterOS", help="image name (<=32 bytes)")
    args = parser.parse_args()

    payload = args.input.read_bytes()
    name = args.name.encode("utf-8", "replace")[:32].ljust(32, b"\0")
    load = int(args.load, 16)
    entry = int(args.entry, 16)
    if load > 0xFFFFFFFF or entry > 0xFFFFFFFF:
        parser.error("legacy uImage load/entry fields are 32-bit; use the physical address")

    def header(hcrc: int, dcrc: int) -> bytes:
        return (
            struct.pack(
                ">IIIIIII",
                IH_MAGIC,
                hcrc,
                0,
                len(payload),
                load,
                entry,
                dcrc,
            )
            + struct.pack(">BBBB", IH_OS_LINUX, IH_ARCH_LOONGARCH, IH_TYPE_KERNEL, IH_COMP_NONE)
            + name
        )

    dcrc = zlib.crc32(payload) & 0xFFFFFFFF
    hcrc = zlib.crc32(header(0, dcrc)) & 0xFFFFFFFF
    assert len(header(hcrc, dcrc)) == HEADER_SIZE
    args.output.write_bytes(header(hcrc, dcrc) + payload)
    print(
        f"wrote {args.output} ({len(payload)} data bytes, "
        f"load={load:#x} entry={entry:#x})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
