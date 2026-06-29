#!/usr/bin/env python3
"""从 docs/technical_document/test.tex 提取各章正文到 wateros-latex/chapters/。"""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEST = ROOT.parent / "test.tex"
CHAPTERS = ROOT / "chapters"

MARKERS = [
    ("chap01.tex", r"\\chapter\{项目概述\}", r"\\chapter\{总体架构设计\}"),
    ("chap02/written-architecture.tex", r"\\chapter\{总体架构设计\}", r"\\chapter\{关键模块实现\}"),
    ("chap03/written-implementation.tex", r"\\chapter\{关键模块实现\}", r"\\chapter\{测试、复现与问题处理\}"),
    ("chap04.tex", r"\\chapter\{测试、复现与问题处理\}", r"\\chapter\{总结与后续工作\}"),
    ("chap05.tex", r"\\chapter\{总结与后续工作\}", r"\\end\{document\}"),
]


def extract_between(text: str, start_pat: str, end_pat: str) -> str:
    m0 = re.search(start_pat, text)
    if not m0:
        raise SystemExit(f"start not found: {start_pat}")
    m1 = re.search(end_pat, text[m0.start() + 1 :])
    if not m1:
        raise SystemExit(f"end not found: {end_pat}")
    chunk = text[m0.start() : m0.start() + 1 + m1.start()]
    return chunk.rstrip() + "\n"


def main() -> None:
    text = TEST.read_text(encoding="utf-8")
    for rel, start, end in MARKERS:
        body = extract_between(text, start, end)
        out = CHAPTERS / rel
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(body, encoding="utf-8")
        print(f"wrote {rel} ({len(body.splitlines())} lines)")


if __name__ == "__main__":
    main()
