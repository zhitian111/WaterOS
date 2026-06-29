#!/usr/bin/env python3
"""按 oscomp 官方 judge 口径分析 LTP 日志（Summary: passed 之和）。

用法:
  python3 ltp_log_final/.agent/analyze_logs.py
  python3 ltp_log_final/.agent/analyze_logs.py ltp_log_final/verify_W0-A.log
  python3 ltp_log_final/.agent/analyze_logs.py --tier0   # 列出 skip 表内可 unskip 项

官方 LTP 计入总分: 500 * log10(1 + 9 * raw / 10000)，raw = 各用例 Summary passed 之和。
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from collections import defaultdict
from pathlib import Path

ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")
MAX_PASSED_SANE = 5000
SKIP_RS = Path("components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ltp_cgroup_helper.rs")


def strip_ansi(s: str) -> str:
    return ANSI_RE.sub("", s)


def parse_skip_names() -> set[str]:
    text = SKIP_RS.read_text(encoding="utf-8")
    m = re.search(r"LTP_SUBMIT_SKIP_BASENAMES.*?&\[(.*?)\n\];", text, re.S)
    if not m:
        return set()
    return set(re.findall(r'"([^"]+)"', m.group(1)))


def parse_cases(text: str) -> tuple[dict[str, int], dict[str, dict]]:
    text = strip_ansi(text)
    scores: dict[str, int] = {}
    meta: dict[str, dict] = {}
    for m in re.finditer(r"RUN LTP CASE (\S+)(.*?)(?=RUN LTP CASE |\Z)", text, re.S):
        name = m.group(1)
        block = m.group(2)
        sm = re.search(r"Summary:\s*\n\s*passed\s+(\d+)", block)
        passed = int(sm.group(1)) if sm else 0
        corrupt = passed > MAX_PASSED_SANE
        if corrupt:
            passed = 0
        tpass = block.count("TPASS")
        has_summary = bool(sm) and not corrupt
        scores[name] = max(scores.get(name, 0), passed if has_summary else 0)
        prev = meta.get(name, {})
        meta[name] = {
            "summary_passed": max(prev.get("summary_passed", 0), passed if has_summary else 0),
            "has_summary": prev.get("has_summary", False) or has_summary,
            "tpass": max(prev.get("tpass", 0), tpass),
            "corrupt_summary": prev.get("corrupt_summary", False) or corrupt,
        }
    return scores, meta


def ltp_mapped(raw: float) -> float:
    raw = max(0.0, min(raw, 10000.0))
    return 500.0 * math.log10(1 + 9 * raw / 10000.0)


def merge_logs(paths: list[Path]) -> tuple[dict[str, int], dict[str, dict], list[tuple]]:
    global_scores: dict[str, int] = defaultdict(int)
    global_meta: dict[str, dict] = {}
    per_log = []
    for path in paths:
        text = path.read_text(encoding="utf-8", errors="replace")
        if "RUN LTP CASE" not in strip_ansi(text):
            continue
        scores, meta = parse_cases(text)
        raw = sum(scores.values())
        per_log.append((path.name, raw, sum(1 for v in scores.values() if v > 0)))
        for k, v in scores.items():
            global_scores[k] = max(global_scores[k], v)
        for k, m in meta.items():
            if k not in global_meta:
                global_meta[k] = m
            else:
                global_meta[k]["summary_passed"] = max(
                    global_meta[k]["summary_passed"], m["summary_passed"]
                )
                global_meta[k]["tpass"] = max(global_meta[k]["tpass"], m["tpass"])
                global_meta[k]["has_summary"] = (
                    global_meta[k]["has_summary"] or m["has_summary"]
                )
    return dict(global_scores), global_meta, per_log


def main() -> int:
    ap = argparse.ArgumentParser(description="LTP 官方 judge 口径日志分析")
    ap.add_argument("logs", nargs="*", help="日志路径；默认扫描 ltp_log_final/*.log")
    ap.add_argument("--tier0", action="store_true", help="打印 skip 表内可 unskip 用例")
    ap.add_argument("--json", action="store_true", help="输出 JSON")
    args = ap.parse_args()

    root = Path("ltp_log_final") if Path("ltp_log_final").is_dir() else Path(".")
    if args.logs:
        paths = [Path(p) for p in args.logs]
    else:
        paths = sorted(root.glob("*.log"))

    scores, meta, per_log = merge_logs(paths)
    skip = parse_skip_names()
    raw_total = sum(scores.values())
    tier0 = sorted(
        [(n, scores[n]) for n in scores if n in skip and scores[n] > 0],
        key=lambda x: (-x[1], x[0]),
    )
    pan = sorted(
        [(n, meta[n]["tpass"]) for n in meta if meta[n]["tpass"] > 0 and scores.get(n, 0) == 0],
        key=lambda x: -x[1],
    )

    if args.json:
        print(
            json.dumps(
                {
                    "judge_raw": raw_total,
                    "ltp_mapped": ltp_mapped(raw_total),
                    "tier0": tier0,
                    "pan_only": pan[:50],
                    "per_log": per_log,
                },
                indent=2,
                ensure_ascii=False,
            )
        )
        return 0

    print(f"日志数: {len(per_log)}")
    print(f"可计分用例: {sum(1 for v in scores.values() if v > 0)}")
    print(f"judge raw (Summary passed 之和): {raw_total}")
    print(f"LTP 折算分 (单架构估算): {ltp_mapped(raw_total):.1f}")
    print(f"skip 表内可 unskip (tier0): {len(tier0)} 项, raw={sum(v for _, v in tier0)}")
    print(f"PAN 格式 (TPASS>0 但 judge=0): {len(pan)} 项, TPASS 行 {sum(t for _, t in pan)}")

    if args.tier0:
        print("\n=== tier0 unskip ===")
        for n, v in tier0:
            print(f"  {n}\t{v}")
        return 0

    if len(paths) == 1:
        print(f"\n=== {paths[0].name} ===")
        for n, v in sorted(scores.items(), key=lambda x: -x[1])[:20]:
            if v > 0:
                print(f"  {n}: passed={v}")
        pan_in_log = [n for n in scores if meta.get(n, {}).get("tpass", 0) > 0 and v == 0 for v in [scores.get(n, 0)]]
        if pan:
            print("\n  PAN 格式 (不计分):")
            for n, t in pan[:10]:
                print(f"    {n}: {t} TPASS lines")

    return 0


if __name__ == "__main__":
    sys.exit(main())
