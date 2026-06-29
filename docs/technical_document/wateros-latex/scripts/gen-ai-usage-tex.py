#!/usr/bin/env python3
"""从 docs/exports/ai-usage-inventory.tsv 生成 chap05-ai-inventory.tex。"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[4]
TSV = REPO_ROOT / "docs/exports/ai-usage-inventory.tsv"
OUT = Path(__file__).resolve().parents[1] / "chapters/chap05-ai-inventory.tex"

SNAPSHOT_LINES = 5


def parse_range(spec: str) -> tuple[int, int]:
    spec = spec.strip()
    if "-" in spec:
        a, b = spec.split("-", 1)
        return int(a), int(b)
    n = int(spec)
    return n, n


def pick_snapshot_lines(start: int, end: int, total_lines: int) -> tuple[bool, tuple[int, int], bool]:
    """返回 (show_ellipsis_before, (lo, hi), show_ellipsis_after)，快照尽量取 5 行。"""
    if start > end:
        start, end = end, start
    span = end - start + 1
    if span >= SNAPSHOT_LINES:
        lo, hi = start, start + SNAPSHOT_LINES - 1
    else:
        center = (start + end) // 2
        half = SNAPSHOT_LINES // 2
        lo = max(1, center - half)
        hi = lo + SNAPSHOT_LINES - 1
        if hi > total_lines:
            hi = total_lines
            lo = max(1, hi - SNAPSHOT_LINES + 1)
    lo = max(1, min(lo, total_lines))
    hi = max(lo, min(hi, total_lines))
    show_before = lo > 1
    show_after = hi < total_lines
    return show_before, (lo, hi), show_after


def minted_line(line: str) -> str:
    """minted 逐行写入；避免源码中出现 \\end{rust} 打断环境。"""
    s = line.rstrip("\n")
    return s.replace("\\end{rust}", "\\end \\{rust\\}")


def read_file_lines(path: Path) -> list[str] | None:
    try:
        return path.read_text(encoding="utf-8").splitlines(keepends=True)
    except OSError as e:
        return None


def main() -> int:
    if not TSV.is_file():
        print(f"missing inventory: {TSV}", file=sys.stderr)
        return 1

    rows: list[tuple[int, str, str]] = []
    for raw in TSV.read_text(encoding="utf-8").splitlines():
        if not raw.strip() or raw.startswith("#"):
            continue
        parts = raw.split("\t")
        if len(parts) < 3:
            continue
        rows.append((int(parts[0]), parts[1].strip(), parts[2].strip()))

    chunks: list[str] = [
        "% 由 scripts/gen-ai-usage-tex.py 根据 docs/exports/ai-usage-inventory.tsv 生成。",
        "% 勿手改；更新清单后重新运行本脚本。",
        "",
    ]

    cache: dict[str, list[str] | None] = {}

    for seq, rel_path, range_spec in rows:
        full = REPO_ROOT / rel_path
        if rel_path not in cache:
            cache[rel_path] = read_file_lines(full)
        lines = cache[rel_path]
        start, end = parse_range(range_spec)

        chunks.append(f"\\subsubsection*{{序号{seq}}}")
        chunks.append(f"\\noindent 路径：\\code{{{rel_path}}}\\\\")
        chunks.append(f"\\noindent 行范围：{range_spec}\\\\[0.4em]")
        chunks.append("")

        if lines is None:
            chunks.append("\\noindent\\textit{（源文件不可读或不存在）}\\par")
            chunks.append("")
            continue

        total = len(lines)
        if total == 0:
            chunks.append("\\noindent\\textit{（源文件为空）}\\par")
            chunks.append("")
            continue

        start = max(1, min(start, total))
        end = max(1, min(end, total))
        if start > end:
            start, end = end, start

        show_before, (lo, hi), show_after = pick_snapshot_lines(start, end, total)

        body: list[str] = []
        if show_before:
            body.append("...")
        for i in range(lo, hi + 1):
            body.append(minted_line(lines[i - 1]))
        if show_after:
            body.append("...")

        chunks.append("\\begin{rust}")
        chunks.extend(body)
        chunks.append("\\end{rust}")
        chunks.append("")

    OUT.write_text("\n".join(chunks) + "\n", encoding="utf-8")
    print(f"wrote {len(rows)} entries -> {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
