"""验证双架构 QEMU 参数组装、镜像和图形后端策略。"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

RUN_SCRIPTS = Path(__file__).resolve().parents[1] / "run"
sys.path.insert(0, str(RUN_SCRIPTS))

from qemu_run import QemuConfigError, build_qemu_launch


class QemuRunTests(unittest.TestCase):
    def test_riscv_launch_has_no_bootargs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            launch = build_qemu_launch(
                "rv",
                "pre",
                {
                    "WOS_SMP": "4",
                    "WOS_QEMU_SNAPSHOT": "1",
                    "WOS_QEMU_GDB": "1",
                    "WOS_QEMU_GDB_PORT": "1235",
                    "WOS_SDCARD": "/images/rv-pre.img",
                },
                root=Path(directory),
            )
            self.assertNotIn("-append", launch.argv)
            self.assertFalse(any("wos." in item for item in launch.argv))
            self.assertIn("-snapshot", launch.argv)
            self.assertIn("tcp:127.0.0.1:1235", launch.argv)

    def test_loongarch_launch_has_no_bootargs_or_mailbox(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            launch = build_qemu_launch(
                "la",
                "final",
                {
                    "WOS_SMP": "8",
                    "WOS_SDCARD": "/images/la-final.img",
                },
                root=Path(directory),
            )
            self.assertNotIn("-append", launch.argv)
            self.assertFalse(any("WOSCMD1" in item or item == "loader" for item in launch.argv))
            self.assertFalse(any("wos." in item for item in launch.argv))

    def test_invalid_smp_is_rejected(self) -> None:
        with self.assertRaises(QemuConfigError):
            build_qemu_launch(
                "rv", "pre", {"WOS_SMP": "9", "WOS_SDCARD": "/images/root.img"}
            )
        with self.assertRaises(QemuConfigError):
            build_qemu_launch(
                "rv", "pre", {"WOS_SMP": "0", "WOS_SDCARD": "/images/root.img"}
            )

    def test_empty_sdcard_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(QemuConfigError, "未指定根文件系统镜像"):
                build_qemu_launch(
                    "rv",
                    "pre",
                    {"WOS_KERNEL": "", "WOS_SDCARD": ""},
                    root=root,
                )

    def test_make_selected_image_is_used_without_profile_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            launch = build_qemu_launch(
                "rv",
                "final",
                {"WOS_SDCARD": "images/custom-final.img"},
                root=root,
            )
            self.assertTrue(
                any(
                    f"file={root / 'images/custom-final.img'}" in item
                    for item in launch.argv
                )
            )

    def test_graphics_replaces_nographic_and_adds_gui_devices(self) -> None:
        for arch, gpu, keyboard, tablet in (
            ("rv", "virtio-gpu-device", "virtio-keyboard-device", "virtio-tablet-device"),
            ("la", "virtio-gpu-pci", "virtio-keyboard-pci", "virtio-tablet-pci"),
        ):
            with self.subTest(arch=arch), tempfile.TemporaryDirectory() as directory:
                class Result:
                    stdout = "Available display backend types:\nnone\nsdl\n"

                with patch("qemu_run.subprocess.run", return_value=Result()):
                    launch = build_qemu_launch(
                        arch,
                        "pre",
                        {
                            "WOS_SDCARD": "/images/root.img",
                            "WOS_GRAPHICS": "1",
                            "WOS_QEMU_DISPLAY": "sdl",
                        },
                        root=Path(directory),
                    )
                    self.assertNotIn("-nographic", launch.argv)
                    self.assertIn("-display", launch.argv)
                    self.assertIn("sdl", launch.argv)
                    self.assertIn("-serial", launch.argv)
                    self.assertIn(gpu, launch.argv)
                    self.assertIn(keyboard, launch.argv)
                    self.assertIn(tablet, launch.argv)
                    launch.cleanup()

    def test_graphics_auto_selects_supported_backend(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            class Result:
                stdout = "Available display backend types:\nnone\ncocoa\n"

            with patch("qemu_run.platform.system", return_value="Darwin"), patch(
                "qemu_run.subprocess.run", return_value=Result()
            ):
                launch = build_qemu_launch(
                    "rv",
                    "pre",
                    {
                        "WOS_SDCARD": "/images/root.img",
                        "WOS_GRAPHICS": "1",
                        "WOS_QEMU_DISPLAY": "auto",
                    },
                    root=root,
                )
                self.assertIn("-display", launch.argv)
                # macOS cocoa 后端应附带 zoom-to-fit=on 等比缩放（防黑边）
                display_arg = launch.argv[launch.argv.index("-display") + 1]
                self.assertTrue(display_arg.startswith("cocoa"),
                                f"unexpected display arg: {display_arg!r}")
                self.assertIn("zoom-to-fit=on", display_arg)
                self.assertNotIn("gtk", launch.argv)
                launch.cleanup()

    def test_graphics_defaults_to_disabled(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            launch = build_qemu_launch(
                "rv",
                "pre",
                {"WOS_SDCARD": "/images/root.img"},
                root=Path(directory),
            )
            self.assertIn("-nographic", launch.argv)
            self.assertNotIn("virtio-gpu-device", launch.argv)


if __name__ == "__main__":
    unittest.main()
