#!/usr/bin/env python3
"""验证 syscall-profile TSV 汇总和 Markdown 输出。"""
from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("analyze.py")
SPEC = importlib.util.spec_from_file_location("syscall_profile_analyze", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
ANALYZE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ANALYZE)


class AnalyzeTests(unittest.TestCase):
    def test_parse_and_render(self) -> None:
        source = """# syscall-profile version=1 backend=ecall total=4
S\t56\topenat\t3\t2\t1
S\t63\tread\t1\t1\t0
A\t63\t2\t2^12\t1
V\t56\t2\t0x0000000000080000\t3
P\t56\t1\t3\t1\t2\t0\t0
D\t56\t1\t2^3\t2
PV\t56\t1\t3\t/lib/libc.so.6
X\t0\tregister_failures\t0\tignored_kernel_ecalls\t2
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.txt"
            path.write_text(source, encoding="utf-8")
            profile = ANALYZE.parse(path)
        self.assertEqual(profile["syscalls"][0]["name"], "openat")
        self.assertEqual(profile["paths"][0]["repeats"], 2)
        report = ANALYZE.render(profile, 10)
        self.assertIn("| `openat` | 56 | 3 | 75.00% |", report)
        self.assertIn("66.67%", report)
        self.assertIn("`/lib/libc.so.6`", report)


if __name__ == "__main__":
    unittest.main()
