"""验证调试快照报告、构建标识和归档输出。"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

DEBUG_SCRIPTS = Path(__file__).resolve().parents[1] / "debug"
sys.path.insert(0, str(DEBUG_SCRIPTS))

import wateros_debug
from wateros_debug import DebugToolError, RemoteSample, verify_build_id, write_report


class BuildIdTests(unittest.TestCase):
    def test_matching_build_id(self) -> None:
        verify_build_id("same", "same")

    def test_mismatched_build_id_is_fatal(self) -> None:
        with self.assertRaises(DebugToolError):
            verify_build_id("local", "remote")


class ReportTests(unittest.TestCase):
    def test_report_contains_all_archive_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report = root / "report"
            report.mkdir()
            elf = root / "kernel-rv-gdb"
            elf.write_bytes(b"test-elf")
            serial = root / "serial.log"
            serial.write_text("line one\nline two\n")
            sample = RemoteSample(
                "T05",
                [],
                {"cpus": [], "events": [], "event_meta": []},
                "test-build",
            )

            def check_output(argv, **kwargs):
                del kwargs
                if argv[:3] == ["git", "rev-parse", "HEAD"]:
                    return "0123456789abcdef\n"
                if argv[:3] == ["git", "status", "--porcelain"]:
                    return ""
                if argv[-1] == "--version":
                    return "GNU gdb test\n"
                raise AssertionError(argv)

            def fake_gdb(_elf, _host, _port, output, *, leave_stopped):
                self.assertTrue(leave_stopped)
                output.write_text("thread apply all bt full\n")

            with (
                patch.object(wateros_debug, "report_directory", return_value=report),
                patch.object(wateros_debug, "gdb_command", return_value="gdb-multiarch"),
                patch.object(wateros_debug, "run_full_gdb", side_effect=fake_gdb),
                patch.object(wateros_debug.subprocess, "check_output", side_effect=check_output),
            ):
                result = write_report(
                    "rv",
                    elf,
                    sample,
                    "manual-snapshot",
                    "127.0.0.1",
                    1234,
                    serial_log=serial,
                )

            self.assertEqual(result, report)
            expected = {
                "summary.txt",
                "metadata.json",
                "snapshot.json",
                "events.json",
                "gdb.txt",
                "serial.log",
                "serial-tail.txt",
                "reproduce.txt",
            }
            self.assertEqual({path.name for path in report.iterdir()}, expected)
            metadata = json.loads((report / "metadata.json").read_text())
            self.assertEqual(metadata["build_id"], "test-build")
            self.assertIn("manual-snapshot", (report / "summary.txt").read_text())


if __name__ == "__main__":
    unittest.main()
