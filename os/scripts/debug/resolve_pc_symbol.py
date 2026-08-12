#!/usr/bin/env python3
"""将 guest PC 地址解析为内核 ELF 符号和源码位置。"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

_SCRIPTS_DIR = Path(__file__).resolve().parent.parent
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

from source.logging_utils import error as log_error  # noqa: E402
from source.argparse_utils import ChineseArgumentParser  # noqa: E402

_DEBUG_DIR = Path(__file__).resolve().parent
if str(_DEBUG_DIR) not in sys.path:
    sys.path.insert(0, str(_DEBUG_DIR))

from symbol_index import SymbolIndex  # noqa: E402


def parse_addr(text: str) -> int:
    text = text.strip().lower()
    if text.startswith("0x"):
        return int(text, 16)
    return int(text, 10)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = ChineseArgumentParser(
        description="将 Guest PC 地址解析为 WaterOS 内核符号和源码位置"
    )
    parser.add_argument("--arch", choices=["rv", "la"], required=True, help="Guest 架构")
    parser.add_argument("--elf", type=Path, required=True, help="内核 ELF 路径")
    parser.add_argument("addresses", nargs="+", help="一个或多个 PC 地址，例如 0x80201234")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if not args.elf.is_file():
        log_error(f"ELF 文件不存在 path={args.elf}", component="SYMBOL")
        return 2

    index = SymbolIndex(args.elf.resolve(), args.arch)  # type: ignore[arg-type]
    exit_code = 0
    for i, addr_text in enumerate(args.addresses):
        try:
            pc = parse_addr(addr_text)
        except ValueError:
            log_error(f"地址格式无效 value={addr_text}", component="SYMBOL")
            exit_code = 1
            continue
        if i > 0:
            print()
        result = index.lookup(pc)
        print(result.format_detail())
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
