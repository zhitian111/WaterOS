from __future__ import annotations

import hashlib
import tempfile
import unittest
from pathlib import Path

from tools import setup


class SetupTests(unittest.TestCase):
    def test_riscv_release_is_pinned(self) -> None:
        release = setup.release_for("rv")
        self.assertEqual(release.compiler_prefix, "riscv64-buildroot-linux-musl-")
        self.assertEqual(len(release.sha256), 64)
        int(release.sha256, 16)

    def test_loongarch_managed_package_set_is_complete(self) -> None:
        self.assertIn("gcc-14-loongarch64-linux-gnu", setup.LA_DEBIAN_PACKAGES)
        self.assertIn("libc6-dev-loong64-cross", setup.LA_DEBIAN_PACKAGES)
        self.assertEqual(setup.LA_COMPILER_PREFIX, "loongarch64-linux-gnu-")

    def test_archive_checksum_verification(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "toolchain.tar.xz"
            archive.write_bytes(b"archive")
            release = setup.ToolchainRelease("rv", archive.name, "https://invalid/",
                                             hashlib.sha256(b"archive").hexdigest(),
                                             "riscv64-buildroot-linux-musl-")
            setup.verify_archive(archive, release)
            archive.write_bytes(b"corrupt")
            with self.assertRaisesRegex(setup.SetupError, "checksum mismatch"):
                setup.verify_archive(archive, release)


if __name__ == "__main__":
    unittest.main()
