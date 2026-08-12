#!/usr/bin/env python3
"""汇总 LTP 日志里每个用例 Summary 段中 ``passed`` 后的数字（子测试通过数之和）。

用法:
  python3 os/scripts/testing/ltp_sum_passed.py [log ...]
  python3 os/scripts/testing/ltp_sum_passed.py os/rv_local_run_all.log

默认读取 os/rv_local_run_all.log（若存在），否则 stdin。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

PASSED_RE = re.compile(r"^passed\s+(\d+)\s*$", re.MULTILINE)
TPASS_RE = re.compile(r"TPASS:")
RUN_RE = re.compile(r"^RUN LTP CASE (\S+)", re.MULTILINE)
ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text)


def summarize(text: str) -> dict:
    text = strip_ansi(text)
    passed_vals = [int(m.group(1)) for m in PASSED_RE.finditer(text)]
    runs = RUN_RE.findall(text)
    return {
        "summary_blocks": len(passed_vals),
        "passed_sum": sum(passed_vals),
        "passed_max": max(passed_vals) if passed_vals else 0,
        "run_cases": len(runs),
        "unique_run_cases": len(set(runs)),
        "tpass_lines": len(TPASS_RE.findall(text)),
    }


def main() -> int:
    paths: list[Path]
    if len(sys.argv) > 1:
        paths = [Path(p) for p in sys.argv[1:]]
    else:
        default = Path("os/rv_local_run_all.log")
        paths = [default] if default.exists() else []

    if not paths:
        text = sys.stdin.read()
        stats = summarize(text)
        print(f"Summary 块数:     {stats['summary_blocks']}")
        print(f"passed 合计:      {stats['passed_sum']}")
        print(f"RUN LTP CASE:     {stats['run_cases']} (唯一 {stats['unique_run_cases']})")
        print(f"TPASS: 行数:      {stats['tpass_lines']}")
        return 0

    grand = {
        "summary_blocks": 0,
        "passed_sum": 0,
        "run_cases": 0,
        "unique_run_cases": 0,
        "tpass_lines": 0,
    }
    for path in paths:
        if not path.is_file():
            print(f"{path}: 文件不存在", file=sys.stderr)
            return 1
        stats = summarize(path.read_text(encoding="utf-8", errors="replace"))
        print(f"=== {path} ===")
        print(f"Summary 块数:     {stats['summary_blocks']}")
        print(f"passed 合计:      {stats['passed_sum']}")
        print(f"单块 passed 最大: {stats['passed_max']}")
        print(f"RUN LTP CASE:     {stats['run_cases']} (唯一 {stats['unique_run_cases']})")
        print(f"TPASS: 行数:      {stats['tpass_lines']}  （仅供参考；官方计分见 passed 合计）")
        print()
        for k in grand:
            if k == "unique_run_cases":
                continue
            grand[k] += stats[k]

    if len(paths) > 1:
        print("=== 合计 ===")
        print(f"Summary 块数:     {grand['summary_blocks']}")
        print(f"passed 合计:      {grand['passed_sum']}")
        print(f"RUN LTP CASE:     {grand['run_cases']}")
        print(f"TPASS: 行数:      {grand['tpass_lines']}  （仅供参考）")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
