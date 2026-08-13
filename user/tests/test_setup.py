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
        self.assertIn("libgcc-s1-loong64-cross", setup.LA_DEBIAN_PACKAGES)
        self.assertEqual(setup.LA_COMPILER_PREFIX, "loongarch64-linux-gnu-")

    def test_loongarch_launchers_use_private_host_libraries(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            tools = root / "usr/bin"
            tools.mkdir(parents=True)
            (tools / "loongarch64-linux-gnu-gcc-14").touch()
            for tool in ("ar", "as", "ld", "nm", "objcopy", "objdump", "ranlib",
                         "readelf", "size", "strings", "strip"):
                (tools / f"loongarch64-linux-gnu-{tool}").touch()

            prefix = setup.create_loongarch_launchers(root)

            gcc = Path(f"{prefix}gcc").read_text(encoding="utf-8")
            assembler = Path(f"{prefix}as").read_text(encoding="utf-8")
            private_lib = 'LD_LIBRARY_PATH="$root/usr/lib/x86_64-linux-gnu'
            self.assertIn(private_lib, gcc)
            self.assertIn(private_lib, assembler)
            self.assertIn('--sysroot="$root"', gcc)

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
