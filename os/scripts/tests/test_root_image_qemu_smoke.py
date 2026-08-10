from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "root_image"))

from qemu_smoke import SmokeError, build_smoke_command  # noqa: E402


class RootImageQemuSmokeTests(unittest.TestCase):
    def test_build_command_forces_snapshot_and_image(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "root.img"
            kernel = root / "kernel-rv"
            image.write_bytes(b"image")
            kernel.write_bytes(b"kernel")
            with patch("qemu_smoke.build_qemu_launch") as build:
                build.return_value.argv = [
                    "qemu-system-riscv64", "-drive", f"file={image}", "-snapshot"
                ]
                command = build_smoke_command("rv", "pre", image, kernel, root=root)
            self.assertIn("-snapshot", command)
            self.assertIn(f"file={image}", command)

    def test_missing_kernel_is_rejected_before_qemu(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "root.img"
            image.write_bytes(b"image")
            with self.assertRaisesRegex(SmokeError, "kernel artifact"):
                build_smoke_command("rv", "pre", image, root / "missing", root=root)


if __name__ == "__main__":
    unittest.main()
