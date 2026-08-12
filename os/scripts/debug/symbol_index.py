#!/usr/bin/env python3
"""为 WaterOS 内核提供 ELF 符号索引和 addr2line 查询。"""
from __future__ import annotations

import bisect
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from pc_trace_parser import Arch, normalize_pc

ArchTooling = dict[str, str]

ARCH_TOOLS: dict[Arch, ArchTooling] = {
    "rv": {
        "nm": "riscv64-unknown-elf-nm",
        "addr2line": "riscv64-unknown-elf-addr2line",
    },
    "la": {
        "nm": "loongarch64-linux-gnu-nm",
        "addr2line": "loongarch64-linux-gnu-addr2line",
    },
}


def _rust_llvm_tool(name: str) -> str | None:
    """查找当前 rustup 工具链附带的 LLVM binutil。"""
    try:
        sysroot = Path(
            subprocess.check_output(
                ["rustc", "--print", "sysroot"], text=True
            ).strip()
        )
        host = subprocess.check_output(
            ["rustc", "-vV"], text=True
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    host_line = next(
        (line.removeprefix("host: ") for line in host.splitlines()
         if line.startswith("host: ")),
        None,
    )
    if host_line is None:
        return None
    candidate = (
        sysroot / "lib" / "rustlib" / host_line / "bin" / name
    )
    return str(candidate) if candidate.is_file() else None


@dataclass(frozen=True)
class SymbolEntry:
    start: int
    size: int
    name: str
    kind: str

    @property
    def end(self) -> int:
        return self.start + self.size if self.size > 0 else self.start + 1


@dataclass(frozen=True)
class SymbolLookup:
    raw_pc: int
    lookup_pc: int
    region_hint: str | None
    symbol: SymbolEntry | None
    offset: int
    nearest: SymbolEntry | None
    source_file: str | None
    source_line: str | None
    addr2line_func: str | None

    def format_short(self, max_name: int = 48) -> str:
        if self.region_hint and self.symbol is None:
            return f"{self._pc_str()}  [{self.region_hint}]"
        if self.symbol is None:
            near = ""
            if self.nearest is not None:
                near = f" (nearest: {self._trim(self.nearest.name, max_name)})"
            return f"{self._pc_str()}  <unknown>{near}"
        name = self._trim(self.symbol.name, max_name)
        off = f"+{self.offset}" if self.offset else ""
        loc = ""
        if self.source_file and self.source_file != "??":
            loc = f"  ({self.source_file}:{self.source_line or '?'})"
        return f"{self._pc_str()}  {name}{off}{loc}"

    def format_detail(self) -> str:
        lines = [
            f"原始 PC：  0x{self.raw_pc:016x}",
            f"查询 PC：  0x{self.lookup_pc:016x}",
        ]
        if self.region_hint:
            lines.append(f"区域：     {self.region_hint}")
        if self.symbol:
            lines.append(f"符号：     {self.symbol.name}")
            lines.append(
                f"范围：     [0x{self.symbol.start:016x}, 0x{self.symbol.end:016x}) "
                f"(size={self.symbol.size})"
            )
            lines.append(f"偏移：     +{self.offset}")
        else:
            lines.append("符号：     不在任何符号范围内")
            if self.nearest:
                lines.append(
                    f"最近符号： {self.nearest.name} @ 0x{self.nearest.start:016x}"
                )
        if self.addr2line_func:
            lines.append(f"函数：     {self.addr2line_func}")
        if self.source_file:
            lines.append(f"源码：     {self.source_file}:{self.source_line or '?'}")
        return "\n".join(lines)

    def _pc_str(self) -> str:
        return f"0x{self.raw_pc:016x}"

    @staticmethod
    def _trim(name: str, max_len: int) -> str:
        if len(name) <= max_len:
            return name
        return name[: max_len - 3] + "..."


class SymbolIndex:
    """Binary-search symbol table built from ELF via nm."""

    def __init__(self, elf_path: Path, arch: Arch) -> None:
        self.elf_path = elf_path
        self.arch = arch
        configured = ARCH_TOOLS[arch]
        legacy_nm = "riscv64-elf-nm" if arch == "rv" else configured["nm"]
        legacy_addr2line = (
            "riscv64-elf-addr2line" if arch == "rv" else configured["addr2line"]
        )
        self._nm = (shutil.which(configured["nm"])
                    or shutil.which(legacy_nm)
                    or _rust_llvm_tool("llvm-nm")
                    or shutil.which("nm")
                    or configured["nm"])
        self._addr2line_tool = (shutil.which(configured["addr2line"])
                                or shutil.which(legacy_addr2line)
                                or _rust_llvm_tool("llvm-addr2line")
                                or shutil.which("addr2line"))
        self._symbols = self._load_symbols()
        self._starts = [s.start for s in self._symbols]

    def lookup(self, raw_pc: int, *, with_source: bool = True) -> SymbolLookup:
        lookup_pc, region_hint = normalize_pc(self.arch, raw_pc)
        symbol, offset = self._find_symbol(lookup_pc)
        nearest = self._find_nearest(lookup_pc)
        func, src_file, src_line = (None, None, None)
        if with_source:
            func, src_file, src_line = self._addr2line(lookup_pc)
        return SymbolLookup(
            raw_pc=raw_pc,
            lookup_pc=lookup_pc,
            region_hint=region_hint if symbol is None else None,
            symbol=symbol,
            offset=offset,
            nearest=nearest,
            source_file=src_file,
            source_line=src_line,
            addr2line_func=func,
        )

    def lookup_fast(self, raw_pc: int) -> SymbolLookup:
        """Symbol lookup without addr2line (safe for high-frequency TUI updates)."""
        return self.lookup(raw_pc, with_source=False)

    def _load_symbols(self) -> list[SymbolEntry]:
        cmd = [
            self._nm,
            "--print-size",
            "--size-sort",
            "--radix=x",
            str(self.elf_path),
        ]
        try:
            out = subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL)
        except (subprocess.CalledProcessError, FileNotFoundError) as exc:
            raise RuntimeError(f"无法对 {self.elf_path} 执行 nm：{exc}") from exc

        symbols: list[SymbolEntry] = []
        for line in out.splitlines():
            parts = line.split(None, 3)
            if len(parts) < 4:
                continue
            addr_s, size_s, sym_type, name = parts
            if sym_type not in "TtDdBbRr":
                continue
            try:
                start = int(addr_s, 16)
                size = int(size_s, 16)
            except ValueError:
                continue
            if size == 0:
                size = 1
            symbols.append(
                SymbolEntry(start=start, size=size, name=name, kind=sym_type)
            )

        symbols.sort(key=lambda s: s.start)
        merged: list[SymbolEntry] = []
        for sym in symbols:
            if merged and sym.start == merged[-1].start:
                if sym.size > merged[-1].size:
                    merged[-1] = sym
            else:
                merged.append(sym)
        return merged

    def _find_symbol(self, addr: int) -> tuple[SymbolEntry | None, int]:
        if not self._symbols:
            return None, 0
        idx = bisect.bisect_right(self._starts, addr) - 1
        if idx < 0:
            return None, 0
        sym = self._symbols[idx]
        if sym.start <= addr < sym.end:
            return sym, addr - sym.start
        return None, 0

    def _find_nearest(self, addr: int) -> SymbolEntry | None:
        if not self._symbols:
            return None
        idx = bisect.bisect_right(self._starts, addr) - 1
        if idx >= 0:
            return self._symbols[idx]
        return self._symbols[0]

    def _addr2line(self, addr: int) -> tuple[str | None, str | None, str | None]:
        if self._addr2line_tool is None:
            return None, None, None
        cmd = [
            self._addr2line_tool,
            "-f",
            "-C",
            "-e",
            str(self.elf_path),
            hex(addr),
        ]
        try:
            out = subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL)
        except (subprocess.CalledProcessError, FileNotFoundError):
            return None, None, None
        lines = [ln.strip() for ln in out.splitlines()]
        if len(lines) < 2:
            return None, None, None
        func, loc = lines[0], lines[1]
        if func == "??" and loc == "??:0":
            return None, None, None
        if ":" in loc:
            file_part, line_part = loc.rsplit(":", 1)
            return func, file_part, line_part
        return func, loc, None
