from __future__ import annotations

import json
import argparse
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT_IMAGE = Path(__file__).resolve().parents[1] / "root_image"
sys.path.insert(0, str(ROOT_IMAGE))

from root_image import ImageError, build_image, populate_staging, parse_mbr_sector


class RootImageTests(unittest.TestCase):
    def test_make_targets_forward_manifest_and_size(self) -> None:
        makefile = ROOT_IMAGE.parents[1] / "Makefile"
        text = makefile.read_text(encoding="utf-8")
        self.assertIn("ROOT_IMAGE_MANIFEST ?=", text)
        self.assertIn("ROOT_IMAGE_SIZE_MIB ?= 32", text)
        self.assertIn('--manifest "$(ROOT_IMAGE_MANIFEST)"', text)
        self.assertIn('--size-mib "$(ROOT_IMAGE_SIZE_MIB)"', text)

    def test_parse_single_linux_partition(self) -> None:
        sector = bytearray(512)
        sector[510:512] = b"\x55\xaa"
        sector[446 + 4] = 0x83
        struct.pack_into("<II", sector, 446 + 8, 2048, 63488)
        partitions = parse_mbr_sector(bytes(sector), 32 * 1024 * 1024)
        self.assertEqual(len(partitions), 1)
        self.assertEqual(partitions[0].start_sector, 2048)
        self.assertEqual(partitions[0].sectors, 63488)

    def test_rejects_partition_past_image_and_overlap(self) -> None:
        sector = bytearray(512)
        sector[510:512] = b"\x55\xaa"
        for index, start in enumerate((2048, 4096)):
            offset = 446 + index * 16
            sector[offset + 4] = 0x83
            struct.pack_into("<II", sector, offset + 8, start, 4096)
        with self.assertRaisesRegex(ImageError, "overlapping"):
            parse_mbr_sector(bytes(sector), 8 * 1024 * 1024)
        struct.pack_into("<II", sector, 446 + 16 + 8, 15000, 4096)
        with self.assertRaisesRegex(ImageError, "beyond"):
            parse_mbr_sector(bytes(sector), 8 * 1024 * 1024)

    def test_manifest_populates_modes_and_rejects_escape(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / "manifest.json"
            manifest.write_text(
                json.dumps(
                    {
                        "directories": [{"path": "/tmp", "mode": "1777"}],
                        "files": [
                            {"path": "/etc/release", "mode": "0644", "content": "ok\n"}
                        ],
                    }
                ),
                encoding="utf-8",
            )
            staging = root / "staging"
            paths = populate_staging(manifest, staging)
            self.assertEqual(paths, ["/tmp", "/etc/release"])
            self.assertEqual((staging / "etc/release").read_text(), "ok\n")
            self.assertEqual((staging / "tmp").stat().st_mode & 0o7777, 0o1777)
            manifest.write_text(
                json.dumps({"files": [{"path": "/../escape", "content": "bad"}]}),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ImageError, "unsafe"):
                populate_staging(manifest, staging)

    def test_failed_force_build_preserves_existing_image(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "root.img"
            output.write_bytes(b"known-good")
            manifest = root / "manifest.json"
            manifest.write_text('{"directories": [], "files": []}', encoding="utf-8")
            args = argparse.Namespace(
                output=output,
                manifest=manifest,
                size_mib=16,
                start_sector=2048,
                uuid="574f5300-0000-4000-8000-000000000001",
                label="WATEROS_ROOT",
                force=True,
            )
            with patch("root_image.make_partition_table", side_effect=ImageError("injected")):
                with self.assertRaisesRegex(ImageError, "injected"):
                    build_image(args)
            self.assertEqual(output.read_bytes(), b"known-good")
            self.assertEqual(list(root.glob(".root.img.*.tmp")), [])


if __name__ == "__main__":
    unittest.main()
