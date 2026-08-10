from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from qemu_input_guest_smoke import diagnose_inputs, missing_markers


class QemuInputGuestSmokeTests(unittest.TestCase):
    def test_marker_parser_requires_keyboard_tablet_registry_and_devfs(self):
        serial = (
            "registered virtio-input #0 kind=Keyboard\n"
            "registered virtio-input #1 kind=Pointer\n"
            "input devices registered: count=2\n"
            "devfs refreshed input=2\n"
        )
        self.assertEqual(missing_markers(serial), [])
        self.assertIn("input=2", missing_markers(serial[: serial.index("input=2")]))

    def test_preflight_accepts_small_fixtures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            kernel = root / "kernel"
            image = root / "root.img"
            kernel.write_bytes(b"K")
            image.write_bytes(b"I")
            self.assertEqual(diagnose_inputs("rv", kernel, image), [])


if __name__ == "__main__":
    unittest.main()
