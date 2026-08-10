from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "root_image"))

from qemu_smoke import (  # noqa: E402
    SmokeError,
    build_smoke_command,
    parse_aux_mount_evidence,
    parse_root_mount_evidence,
    run_smoke,
)


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
            environment = build.call_args.args[2]
            self.assertEqual(environment["WOS_QEMU_MEM"], "256M")

    def test_loongarch_smoke_uses_one_gibibyte_default(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "root.img"
            kernel = root / "kernel-la"
            image.write_bytes(b"image")
            kernel.write_bytes(b"kernel")
            with patch("qemu_smoke.build_qemu_launch") as build:
                build.return_value.argv = [
                    "qemu-system-loongarch64", "-drive", f"file={image}", "-snapshot"
                ]
                build_smoke_command("la", "pre", image, kernel, root=root)
            self.assertEqual(build.call_args.args[2]["WOS_QEMU_MEM"], "1G")

    def test_missing_kernel_is_rejected_before_qemu(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / "root.img"
            image.write_bytes(b"image")
            with self.assertRaisesRegex(SmokeError, "kernel artifact"):
                build_smoke_command("rv", "pre", image, root / "missing", root=root)

    def test_root_mount_parser_distinguishes_success_failure_and_absence(self) -> None:
        success = parse_root_mount_evidence(
            "[bringup][stage-00-bus] ext4 root mounted (RW)"
        )
        self.assertEqual(success.state, "success")
        failure = parse_root_mount_evidence("[bringup][stage-00-bus] root mount failed: no device")
        self.assertEqual(failure.state, "failure")
        self.assertEqual(parse_root_mount_evidence("booting kernel").state, "absent")
        self.assertEqual(parse_aux_mount_evidence(
            "[bringup][stage-00-bus] aux ext4 mounted block=/dev/vda2 at /data ro=true"
        ).state, "success")
        self.assertEqual(parse_aux_mount_evidence(
            "[bringup][stage-00-bus] aux mount failed block=/dev/vda2"
        ).state, "failure")

    @patch("qemu_smoke.subprocess.run")
    def test_strict_smoke_requires_successful_mount_evidence(self, run) -> None:
        run.return_value = type(
            "Completed", (), {"returncode": 0, "stdout": "[fs::rootfs] mount root RW from /dev/vda1\n"}
        )()
        self.assertEqual(
            run_smoke(["qemu-system-riscv64"], root=Path("."), timeout=1, require_root_mount=True),
            0,
        )
        run.return_value.stdout = "booting kernel\n"
        with self.assertRaisesRegex(SmokeError, "evidence missing"):
            run_smoke(["qemu-system-riscv64"], root=Path("."), timeout=1, require_root_mount=True)

    @patch("qemu_smoke.subprocess.run")
    def test_aux_mount_evidence_is_optional_but_strict_when_requested(self, run) -> None:
        run.return_value = type(
            "Completed", (), {"returncode": 0,
                               "stdout": "[bringup][stage-00-bus] aux ext4 mounted block=/dev/vda2 at /data ro=true\n"}
        )()
        self.assertEqual(
            run_smoke(["qemu-system-riscv64"], root=Path("."), timeout=1,
                      require_aux_mount=True), 0
        )

    @patch("qemu_smoke.subprocess.run")
    def test_long_running_kernel_can_succeed_after_collecting_mount_evidence(self, run) -> None:
        import subprocess

        run.side_effect = subprocess.TimeoutExpired(
            ["qemu-system-riscv64"], 1,
            output=("[fs::rootfs] mount root RW from /dev/vda1\n"
                    "[bringup][stage-00-bus] aux ext4 mounted block=/dev/vda2 at /data ro=true\n"),
        )
        self.assertEqual(
            run_smoke(["qemu-system-riscv64"], root=Path("."), timeout=1,
                      require_root_mount=True, require_aux_mount=True), 0
        )


if __name__ == "__main__":
    unittest.main()
