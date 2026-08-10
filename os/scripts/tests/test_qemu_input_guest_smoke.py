from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from qemu_input_guest_smoke import diagnose_inputs, missing_markers, qmp_input_events


class QemuInputGuestSmokeTests(unittest.TestCase):
    def test_marker_parser_requires_keyboard_tablet_registry_and_devfs(self):
        serial = (
            "registered virtio-input #0 kind=Keyboard\n"
            "registered virtio-input #1 kind=Pointer\n"
            "input devices registered: count=2\n"
            "devfs refreshed input=2\n"
            "[gui] input events received=4\n"
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

    def test_qmp_batch_contains_key_and_absolute_tablet_events(self):
        events = qmp_input_events()
        self.assertEqual([event["type"] for event in events], ["key", "key", "abs", "abs"])
        self.assertEqual(events[0]["data"]["key"]["data"], "a")
        self.assertEqual({event["data"]["axis"] for event in events[2:]}, {"x", "y"})


if __name__ == "__main__":
    unittest.main()
