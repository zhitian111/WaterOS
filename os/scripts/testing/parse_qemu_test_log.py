#!/usr/bin/env python3
"""解析 WaterOS QEMU bring-up 日志，并汇总各测试组结果。"""
import argparse
import re
import sys
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))
from source.argparse_utils import ChineseArgumentParser  # noqa: E402

def strip_ansi(s: str) -> str:
    return re.sub(r"\x1b\[[0-9;]*m", "", s)

def parse_basic(section: str) -> dict:
    results = {}
    parts = re.split(r"\nTesting (\S+) :", section)
    for i in range(1, len(parts), 2):
        name = parts[i]
        content = parts[i + 1] if i + 1 < len(parts) else ""
        if "Assert Fatal" in content:
            results[name] = "FAIL"
        elif re.search(r"========== END", content):
            results[name] = "PASS"
        else:
            results[name] = "UNKNOWN"
    passed = sum(1 for v in results.values() if v == "PASS")
    return {"kind": "basic", "total": len(results), "passed": passed, "details": results}

def parse_busybox(section: str) -> dict:
    success = len(re.findall(r"testcase busybox .+ success", section))
    fail = len(re.findall(r"testcase busybox .+ fail", section))
    return {"kind": "busybox", "passed": success, "failed": fail, "total": success + fail}

def parse_libctest(section: str) -> dict:
    passes = len(re.findall(r"\nPass!\n", section))
    starts = len(re.findall(r"========== START", section))
    return {"kind": "libctest", "passed": passes, "started": starts}

def parse_ltp(section: str) -> dict:
    cases = re.findall(r"FAIL LTP CASE (\S+) : (-?\d+)", section)
    fail_nz = sum(1 for _, ret in cases if ret != "0")
    pass_nz = sum(1 for _, ret in cases if ret == "0")
    return {"kind": "ltp", "passed": pass_nz, "failed": fail_nz, "total": len(cases)}

def parse_generic(section: str) -> dict:
    start = section.find("#### OS COMP TEST GROUP START")
    end = section.find("#### OS COMP TEST GROUP END")
    has_start = start >= 0
    has_end = end >= 0
    faults = len(re.findall(r"killing user task", section))
    assert_fatal = section.count("Assert Fatal")
    return {
        "kind": "generic",
        "started": has_start,
        "ended": has_end,
        "faults": faults,
        "assert_fatal": assert_fatal,
    }

def classify_script(path: str) -> str:
    for key in ("basic", "busybox", "libctest", "ltp", "lua", "lmbench", "unixbench",
                "libcbench", "iozone", "iperf", "netperf", "cyclictest"):
        if key in path:
            return key
    return "other"

def main() -> int:
    parser = ChineseArgumentParser(description=__doc__)
    parser.add_argument(
        "log",
        nargs="?",
        default="/tmp/wateros_full_run.log",
        help="QEMU bring-up 日志路径，默认为 /tmp/wateros_full_run.log",
    )
    args = parser.parse_args()
    log_path = args.log
    text = strip_ansi(open(log_path, "r", errors="replace").read())

    scripts = re.split(r"\[busybox-bringup\] script_path = ", text)[1:]
    rows = []
    for block in scripts:
        path = block.split("\n", 1)[0].strip()
        exit_m = re.search(r"\[busybox-bringup\] END path=.*? exit_code=(-?\d+)", block)
        if not exit_m:
            exit_m = re.search(r"exit_code=(-?\d+)", block)
        exit_code = int(exit_m.group(1)) if exit_m else None

        start_m = re.search(r"#### OS COMP TEST GROUP START ([^-]+?) ####", block)
        group = start_m.group(1).strip() if start_m else classify_script(path)

        if start_m:
            end_pat = rf"#### OS COMP TEST GROUP END {re.escape(group)} ####"
            end_m = re.search(end_pat, block)
            section = block[start_m.start(): end_m.end()] if end_m else block[start_m.start():]
        else:
            section = block

        kind = classify_script(path)
        if kind == "basic":
            detail = parse_basic(section)
        elif kind == "busybox":
            detail = parse_busybox(section)
        elif kind == "libctest":
            detail = parse_libctest(section)
        elif kind == "ltp":
            detail = parse_ltp(section)
        else:
            detail = parse_generic(section)

        rows.append((path, exit_code, group, detail))

    print(f"{'SCRIPT':<42} {'EXIT':>5}  SUMMARY")
    print("-" * 80)
    for path, exit_code, group, detail in rows:
        exit_s = str(exit_code) if exit_code is not None else "?"
        k = detail["kind"]
        if k == "basic":
            summary = f"{detail['passed']}/{detail['total']} pass"
        elif k == "busybox":
            summary = f"通过 {detail['passed']}，失败 {detail['failed']}，总计 {detail['total']}"
        elif k == "libctest":
            summary = f"{detail['passed']} Pass! ({detail['started']} started)"
        elif k == "ltp":
            summary = f"通过 {detail['passed']}，失败 {detail['failed']}，总计 {detail['total']}"
        else:
            summary = (
                f"START={'Y' if detail['started'] else 'N'} END={'Y' if detail['ended'] else 'N'}"
                f" faults={detail['faults']} assert={detail['assert_fatal']}"
            )
        print(f"{path:<42} {exit_s:>5}  {summary}")

    return 0

if __name__ == "__main__":
    raise SystemExit(main())
