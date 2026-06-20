#!/usr/bin/env python3
"""Resolve a guest PC address to kernel ELF symbol and source location."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

_DEBUG_DIR = Path(__file__).resolve().parent / "debug"
if str(_DEBUG_DIR) not in sys.path:
    sys.path.insert(0, str(_DEBUG_DIR))

from symbol_index import SymbolIndex  # noqa: E402


def parse_addr(text: str) -> int:
    text = text.strip().lower()
    if text.startswith("0x"):
        return int(text, 16)
    return int(text, 10)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Resolve guest PC to WaterOS kernel symbol (rv/la)"
    )
    parser.add_argument("--arch", choices=["rv", "la"], required=True)
    parser.add_argument("--elf", type=Path, required=True, help="Path to kernel ELF")
    parser.add_argument("addresses", nargs="+", help="PC address(es), e.g. 0x80201234")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if not args.elf.is_file():
        print(f"error: ELF not found: {args.elf}", file=sys.stderr)
        return 2

    index = SymbolIndex(args.elf.resolve(), args.arch)  # type: ignore[arg-type]
    exit_code = 0
    for i, addr_text in enumerate(args.addresses):
        try:
            pc = parse_addr(addr_text)
        except ValueError:
            print(f"error: invalid address: {addr_text}", file=sys.stderr)
            exit_code = 1
            continue
        if i > 0:
            print()
        result = index.lookup(pc)
        print(result.format_detail())
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
