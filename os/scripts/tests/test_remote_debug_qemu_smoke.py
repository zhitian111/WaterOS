from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from remote_debug_qemu_smoke import diagnose_inputs, monitor_listening_seen, qemu_binary_name


class RemoteDebugQemuSmokeTests(unittest.TestCase):
    def test_architecture_maps_to_qemu_binary(self):
        self.assertEqual(qemu_binary_name("rv"), "qemu-system-riscv64")
        self.assertEqual(qemu_binary_name("la"), "qemu-system-loongarch64")

    def test_preflight_reports_missing_and_empty_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            kernel = root / "kernel"
            sdcard = root / "sdcard"
            kernel.touch()
            sdcard.touch()
            errors = diagnose_inputs("rv", kernel, sdcard, 0)
            self.assertTrue(any("kernel is empty" in error for error in errors))
            self.assertTrue(any("sdcard is empty" in error for error in errors))
            self.assertTrue(any("port must" in error for error in errors))

    def test_preflight_accepts_small_nonempty_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            kernel = root / "kernel"
            sdcard = root / "sdcard"
            kernel.write_bytes(b"K")
            sdcard.write_bytes(b"S")
            self.assertEqual(diagnose_inputs("la", kernel, sdcard, 22323), [])

    def test_serial_diagnostic_distinguishes_guest_listen_from_feature_gap(self):
        self.assertTrue(
            monitor_listening_seen(
                "[remote-debug] unauthenticated development monitor listening on tcp/2323"
            )
        )
        self.assertFalse(monitor_listening_seen("network stack initialized"))


if __name__ == "__main__":
    unittest.main()
