from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from root_image_qemu_smoke import inspect_with_qemu_img


class RootImageQemuSmokeTests(unittest.TestCase):
    def test_qemu_img_json_is_checked(self):
        class Result:
            stdout = '{"format":"raw","virtual-size":16777216}'

        with patch("root_image_qemu_smoke.subprocess.run", return_value=Result()) as run:
            with tempfile.TemporaryDirectory() as directory:
                info = inspect_with_qemu_img(Path(directory) / "root.img", "qemu-img-test")
        self.assertEqual(info["format"], "raw")
        run.assert_called_once()

    def test_non_raw_image_is_rejected(self):
        class Result:
            stdout = '{"format":"qcow2","virtual-size":16777216}'

        with patch("root_image_qemu_smoke.subprocess.run", return_value=Result()):
            with tempfile.TemporaryDirectory() as directory:
                with self.assertRaisesRegex(RuntimeError, "unexpected image format"):
                    inspect_with_qemu_img(Path(directory) / "root.img")


if __name__ == "__main__":
    unittest.main()
