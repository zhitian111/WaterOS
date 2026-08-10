import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from qemu_rootfs_guest_smoke import REQUIRED_MARKERS, diagnose_inputs, missing_markers


class QemuRootfsGuestSmokeTests(unittest.TestCase):
    def test_marker_parser_requires_rootfs_chain(self):
        serial = "\n".join(REQUIRED_MARKERS)
        self.assertEqual(missing_markers(serial), [])
        self.assertEqual(missing_markers(serial.replace(REQUIRED_MARKERS[2], "")),
                         [REQUIRED_MARKERS[2]])

    def test_diagnose_inputs_reports_missing_artifacts(self):
        errors = diagnose_inputs("rv", Path("/missing/kernel"), Path("/missing/root.img"))
        self.assertEqual(len(errors), 2)
        self.assertIn("kernel not found", errors[0])
        self.assertIn("sdcard not found", errors[1])

    def test_diagnose_inputs_rejects_architecture(self):
        errors = diagnose_inputs("mips", Path("kernel"), Path("root.img"))
        self.assertTrue(errors[0].startswith("unsupported architecture"))


if __name__ == "__main__":
    unittest.main()
