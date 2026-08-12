#!/usr/bin/env python3
"""解析 QEMU exec/nochain trace，并规范化 guest PC 地址。"""
from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Literal

Arch = Literal["rv", "la"]

# QEMU 10.x 输出格式：Trace 0: 0xHOST [flags/GUEST_PC/...]
TRACE_RE = re.compile(
    r"^Trace\s+\d+:\s+0x[0-9a-fA-F]+\s+\[[^/]+/([0-9a-fA-F]+)/",
    re.ASCII,
)
# TB 链停止位置格式：Stopped execution of TB chain before 0xHOST [GUEST_PC]
STOPPED_RE = re.compile(
    r"^Stopped execution of TB chain before 0x[0-9a-fA-F]+\s+\[([0-9a-fA-F]+)\]",
    re.ASCII,
)
# 旧版 QEMU 输出格式：exec ffffffff812f285d
LEGACY_EXEC_RE = re.compile(r"^exec\s+([0-9a-fA-F]+)$", re.ASCII)

# RISC-V link.ld 常量
RV_QEMU_ENTRY = 0x8020_0000
RV_KERNEL_OFFSET = 0xFFFF_FFC0_0000_0000
RV_PHYS_KERNEL_LO = 0x8000_0000
RV_PHYS_KERNEL_HI = 0x8100_0000

# LoongArch link.ld 常量
LA_QEMU_ENTRY = 0x9000_0000
LA_KERNEL_ENTRY_LO = 0x9000_0000
LA_KERNEL_ENTRY_HI = 0xA000_0000
LA_FIRMWARE_LO = 0x1C00_0000
LA_FIRMWARE_HI = 0x1D00_0000


@dataclass(frozen=True)
class ParsedLine:
    """单行 QEMU 输出的解析结果。"""

    is_trace: bool
    pc: int | None = None


# 从批量读取的 trace 二进制块中快速提取全部 Guest PC
TRACE_PC_BYTES_RE = re.compile(
    rb"Trace\s+\d+:\s+0x[0-9a-fA-F]+\s+\[[^/]+/([0-9a-fA-F]+)/",
    re.ASCII,
)


def parse_trace_pcs_from_chunk(data: bytes) -> list[int]:
    """返回原始 trace 数据块中包含的 Guest PC。"""
    return [int(m, 16) for m in TRACE_PC_BYTES_RE.findall(data)]


def parse_qemu_line(line: str) -> ParsedLine:
    """返回 ParsedLine；当输入行包含 Guest PC 时设置 pc。"""
    stripped = line.rstrip("\n\r")
    m = TRACE_RE.match(stripped)
    if m:
        return ParsedLine(is_trace=True, pc=int(m.group(1), 16))
    m = STOPPED_RE.match(stripped)
    if m:
        return ParsedLine(is_trace=True, pc=int(m.group(1), 16))
    m = LEGACY_EXEC_RE.match(stripped)
    if m:
        return ParsedLine(is_trace=True, pc=int(m.group(1), 16))
    return ParsedLine(is_trace=False)


def normalize_pc(arch: Arch, pc: int) -> tuple[int, str | None]:
    """将 Guest PC 映射为 ELF 符号查询地址。

    返回 `(lookup_addr, region_hint)`。地址可直接使用时，`region_hint` 为
    `None`；否则返回简短的区域说明，例如固件地址区间。
    """
    if arch == "rv":
        if pc >= RV_KERNEL_OFFSET:
            return pc - (RV_KERNEL_OFFSET - RV_QEMU_ENTRY), None
        if RV_PHYS_KERNEL_LO <= pc < RV_PHYS_KERNEL_HI:
            return pc, None
        if pc < RV_PHYS_KERNEL_LO or (RV_PHYS_KERNEL_HI <= pc < RV_KERNEL_OFFSET):
            return pc, "固件或内核 ELF 之外的区域，可能属于 OpenSBI"
        return pc, None

    if arch == "la":
        if LA_KERNEL_ENTRY_LO <= pc < LA_KERNEL_ENTRY_HI:
            return pc, None
        if LA_FIRMWARE_LO <= pc < LA_FIRMWARE_HI:
            return pc, "firmware/out of kernel ELF (LoongArch firmware)"
        return pc, "out of kernel ELF"

    return pc, None


def format_pc(pc: int) -> str:
    return f"0x{pc:016x}"
