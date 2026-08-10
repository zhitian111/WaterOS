from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from operator_smoke import diagnose_operator_inputs


class OperatorSmokeTests(unittest.TestCase):
    def test_no_build_requires_nonempty_kernel(self):
        with tempfile.TemporaryDirectory() as directory:
            kernel = Path(directory) / "kernel-rv-pre"
            kernel.touch()
            with patch("operator_smoke.shutil.which", return_value="/usr/bin/qemu"):
                errors = diagnose_operator_inputs("rv", kernel, True)
            self.assertTrue(any("missing or empty" in error for error in errors))

    def test_existing_kernel_and_qemu_pass_preflight(self):
        with tempfile.TemporaryDirectory() as directory:
            kernel = Path(directory) / "kernel-la-pre"
            kernel.write_bytes(b"kernel")
            with patch("operator_smoke.shutil.which", return_value="/usr/bin/qemu"):
                self.assertEqual(diagnose_operator_inputs("la", kernel, True), [])

    def test_missing_qemu_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            kernel = Path(directory) / "kernel-rv-pre"
            kernel.write_bytes(b"kernel")
            with patch("operator_smoke.shutil.which", return_value=None):
                errors = diagnose_operator_inputs("rv", kernel, False)
            self.assertTrue(any("QEMU binary not found" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
