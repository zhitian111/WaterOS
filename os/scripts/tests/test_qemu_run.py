from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from qemu_run import QemuConfigError, build_qemu_launch


class QemuRunTests(unittest.TestCase):
    def test_riscv_run_mode_has_one_bootargs_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            launch = build_qemu_launch(
                "rv",
                "pre",
                {
                    "WOS_MODE": "run",
                    "WOS_SCRIPT": "/root/test.sh",
                    "WOS_SMP": "4",
                    "WOS_QEMU_SNAPSHOT": "1",
                    "WOS_QEMU_GDB": "1",
                    "WOS_QEMU_GDB_PORT": "1235",
                    "WOS_SDCARD": "/images/rv-pre.img",
                },
                root=Path(directory),
            )
            self.assertIn("wos.mode=run wos.script=/root/test.sh", launch.argv)
            self.assertIn("-snapshot", launch.argv)
            self.assertIn("tcp:127.0.0.1:1235", launch.argv)
            self.assertEqual(launch.temporary_files, [])

    def test_loongarch_mailbox_matches_append(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            launch = build_qemu_launch(
                "la",
                "final",
                {
                    "WOS_MODE": "shell",
                    "WOS_SMP": "8",
                    "WOS_SDCARD": "/images/la-final.img",
                },
                root=Path(directory),
            )
            append = launch.argv[launch.argv.index("-append") + 1]
            mailbox = launch.temporary_files[0]
            self.assertEqual(mailbox.read_bytes(), b"WOSCMD1" + append.encode() + b"\0")
            launch.cleanup()
            self.assertFalse(mailbox.exists())

    def test_script_requires_run_mode_and_absolute_path(self) -> None:
        with self.assertRaises(QemuConfigError):
            build_qemu_launch(
                "rv", "pre", {"WOS_MODE": "auto", "WOS_SCRIPT": "/root/x"}
            )
        with self.assertRaises(QemuConfigError):
            build_qemu_launch(
                "rv", "pre", {"WOS_MODE": "run", "WOS_SCRIPT": "root/x"}
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


if __name__ == "__main__":
    unittest.main()
