#!/usr/bin/env python3
"""从 docs/exports/ai-usage-inventory.tsv 生成 chap05-ai-inventory.tex。

按组件列举条目；同一文件的多条标注合并为一行，行范围为该文件内最小行到最大行。
排除 wateros-syscall 组件及路径中含 syscall 实现文件（如 syscall.rs）的条目。
各组件仅附一条代表条目的代码快照。
"""

from __future__ import annotations

import re
import sys
from collections import defaultdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[4]
TSV = REPO_ROOT / "docs/exports/ai-usage-inventory.tsv"
OUT = Path(__file__).resolve().parents[1] / "chapters/chap05-ai-inventory.tex"

SNAPSHOT_LINES = 5
COMPONENT_RE = re.compile(r"os/components/(wateros-[^/]+)/")
EXCLUDED_COMPONENTS = frozenset({"wateros-syscall"})
SYSCALL_PATH_RE = re.compile(r"(?:^|/)syscall(?:_|\.|-)")


def parse_range(spec: str) -> tuple[int, int]:
    spec = spec.strip()
    if "-" in spec:
        a, b = spec.split("-", 1)
        return int(a), int(b)
    n = int(spec)
    return n, n


def span(spec: str) -> int:
    start, end = parse_range(spec)
    return end - start + 1


def component_of(path: str) -> str:
    m = COMPONENT_RE.search(path)
    return m.group(1) if m else "other"


def is_syscall_related(path: str) -> bool:
    comp = component_of(path)
    if comp in EXCLUDED_COMPONENTS:
        return True
    basename = path.rsplit("/", 1)[-1]
    if basename == "syscall.rs" or basename.startswith("syscall_"):
        return True
    if SYSCALL_PATH_RE.search(path):
        return True
    return False


def code_path(path: str) -> str:
    return path.replace("_", "\\_")


def pick_snapshot_lines(start: int, end: int, total_lines: int) -> tuple[bool, tuple[int, int], bool]:
    if start > end:
        start, end = end, start
    line_span = end - start + 1
    if line_span >= SNAPSHOT_LINES:
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
    return lo > 1, (lo, hi), hi < total_lines


def minted_line(line: str) -> str:
    s = line.rstrip("\n")
    return s.replace("\\end{rustcode}", "\\end \\{rustcode\\}")


def read_file_lines(path: Path) -> list[str] | None:
    try:
        return path.read_text(encoding="utf-8").splitlines(keepends=True)
    except OSError:
        return None


def merge_by_file(entries: list[tuple[int, str, str]]) -> list[tuple[str, str]]:
    """同一文件的多条记录合并为一行；行范围取并集。返回顺序按原 TSV 最小序号。"""
    by_path: dict[str, list[tuple[int, str, str]]] = defaultdict(list)
    for entry in entries:
        by_path[entry[1]].append(entry)

    merged: list[tuple[str, str]] = []
    for path in sorted(by_path, key=lambda p: min(e[0] for e in by_path[p])):
        group = by_path[path]
        line_lo = min(parse_range(e[2])[0] for e in group)
        line_hi = max(parse_range(e[2])[1] for e in group)
        range_spec = str(line_lo) if line_lo == line_hi else f"{line_lo}-{line_hi}"
        merged.append((path, range_spec))
    return merged


def renumber_entries(merged: list[tuple[str, str]]) -> list[tuple[int, str, str]]:
    """合并后按组件内从 1 起连续编号。"""
    return [(i, path, range_spec) for i, (path, range_spec) in enumerate(merged, start=1)]


def pick_representative(entries: list[tuple[int, str, str]]) -> tuple[int, str, str]:
    """取行范围跨度最大的一条；并列时取序号最小者。"""
    return min(entries, key=lambda e: (-span(e[2]), e[0]))


def render_snapshot(
    chunks: list[str],
    rel_path: str,
    range_spec: str,
    cache: dict[str, list[str] | None],
) -> None:
    chunks.append(f"\\noindent 路径：\\code{{{code_path(rel_path)}}}\\\\")
    chunks.append(f"\\noindent 行范围：{range_spec}\\\\[0.4em]")
    chunks.append("")

    full = REPO_ROOT / rel_path
    if rel_path not in cache:
        cache[rel_path] = read_file_lines(full)
    lines = cache[rel_path]
    if lines is None:
        chunks.append("\\noindent\\textit{（源文件不可读或不存在）}\\par")
        chunks.append("")
        return
    total = len(lines)
    if total == 0:
        chunks.append("\\noindent\\textit{（源文件为空）}\\par")
        chunks.append("")
        return

    start, end = parse_range(range_spec)
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

    chunks.append("\\begin{rustcode}")
    chunks.extend(body)
    chunks.append("\\end{rustcode}")
    chunks.append("")


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
        path = parts[1].strip()
        if is_syscall_related(path):
            continue
        rows.append((int(parts[0]), path, parts[2].strip()))

    by_comp: dict[str, list[tuple[int, str, str]]] = defaultdict(list)
    for row in rows:
        by_comp[component_of(row[1])].append(row)

    comp_names = sorted(k for k in by_comp if k != "other")
    if "other" in by_comp:
        comp_names.append("other")

    chunks: list[str] = [
        "% 由 scripts/gen-ai-usage-tex.py 根据 docs/exports/ai-usage-inventory.tsv 生成。",
        "% 勿手改；更新清单后重新运行本脚本。",
        "% 已排除 wateros-syscall 及 syscall*.rs 等条目；同文件行范围已合并。",
        "",
    ]
    cache: dict[str, list[str] | None] = {}
    merged_total = 0

    for comp in comp_names:
        entries = renumber_entries(merge_by_file(sorted(by_comp[comp], key=lambda e: e[0])))
        if not entries:
            continue
        merged_total += len(entries)
        rep = pick_representative(entries)
        chunks.append(f"\\subsection*{{\\code{{{comp}}}（{len(entries)}条）}}")
        chunks.append("")
        chunks.append(
            "\\begin{longtable}{>{\\raggedright\\arraybackslash}p{1.2cm}"
            ">{\\raggedright\\arraybackslash}p{9.8cm}"
            ">{\\raggedright\\arraybackslash}p{2.2cm}}"
        )
        chunks.append(f"  \\caption{{{comp} AI标注条目（同文件已合并）}}\\\\")
        chunks.append("  \\toprule[1pt]")
        chunks.append("  序号 & 路径 & 行范围 \\\\")
        chunks.append("  \\midrule[0.6pt]")

        for seq, rel_path, range_spec in entries:
            chunks.append(
                f"  {seq} & \\code{{{code_path(rel_path)}}} & {range_spec} \\\\"
            )

        chunks.append("  \\bottomrule[1pt]")
        chunks.append("\\end{longtable}")
        chunks.append("")
        render_snapshot(chunks, rep[1], rep[2], cache)

    OUT.write_text("\n".join(chunks) + "\n", encoding="utf-8")
    print(
        f"wrote {merged_total} merged rows from {len(rows)} raw rows "
        f"in {len(comp_names)} components -> {OUT}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
