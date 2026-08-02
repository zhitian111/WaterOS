from __future__ import annotations

import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

import wateros_debug


class DoctorTests(unittest.TestCase):
    def run_doctor(self, tools: set[str], elf: Path | None = None) -> tuple[int, str]:
        def which(name: str) -> str | None:
            return f"/usr/bin/{name}" if name in tools else None

        output = io.StringIO()
        with patch.object(wateros_debug.shutil, "which", side_effect=which), redirect_stdout(output):
            result = wateros_debug.doctor("rv", elf)
        return result, output.getvalue()

    def test_complete_host_without_elf_passes(self) -> None:
        result, output = self.run_doctor(
            {
                "gdb-multiarch",
                "readelf",
                "python3",
                "qemu-system-riscv64",
                "nm",
                "addr2line",
            }
        )
        self.assertEqual(result, 0)
        self.assertIn("doctor passed", output)

    def test_missing_gdb_fails_deterministically(self) -> None:
        result, output = self.run_doctor(
            {"readelf", "python3", "qemu-system-riscv64", "nm", "addr2line"}
        )
        self.assertEqual(result, 2)
        self.assertIn("[MISSING] gdb-multiarch", output)

    def test_missing_symbol_tool_fails(self) -> None:
        result, output = self.run_doctor(
            {
                "gdb-multiarch",
                "readelf",
                "python3",
                "qemu-system-riscv64",
                "addr2line",
            }
        )
        self.assertEqual(result, 2)
        self.assertIn("[MISSING] rv nm", output)

    def test_invalid_elf_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            elf = Path(directory) / "kernel-rv-gdb"
            elf.write_bytes(b"not an elf")
            result, output = self.run_doctor(
                {
                    "gdb-multiarch",
                    "readelf",
                    "python3",
                    "qemu-system-riscv64",
                    "nm",
                    "addr2line",
                },
                elf,
            )
        self.assertEqual(result, 2)
        self.assertIn("[INVALID] ELF", output)


if __name__ == "__main__":
    unittest.main()
