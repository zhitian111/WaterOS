from __future__ import annotations

import hashlib
import gzip
import json
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS / "perf"))

import prepare_buildstorm_image as prepare


class PrepareBuildStormImageTests(unittest.TestCase):
    def test_output_names_are_arch_specific(self) -> None:
        root = Path("/output")
        self.assertEqual(prepare.output_image_path(root, "rv"), root / "sdcard-rv-pub-prepared.img")
        self.assertEqual(prepare.output_image_path(root, "la"), root / "sdcard-la-pub-prepared.img")

    def test_debugfs_commands_target_glibc_script_and_set_executable_mode(self) -> None:
        commands = prepare.debugfs_commands(Path("/input/script.sh"))
        self.assertEqual(commands[0], "rm /glibc/buildstorm_testcode.sh")
        self.assertIn("write /input/script.sh /glibc/buildstorm_testcode.sh", commands)
        self.assertIn("mode 0100755", commands[-1])

    def test_validate_hash_accepts_exact_file_and_rejects_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input"
            path.write_bytes(b"wateros")
            expected = hashlib.sha256(b"wateros").hexdigest()
            self.assertEqual(prepare.validate_hash(path, expected, "input"), expected)
            with self.assertRaises(RuntimeError):
                prepare.validate_hash(path, "0" * 64, "input")

    def test_reusable_master_requires_matching_raw_hash(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "master.img"
            image.write_bytes(b"wateros")
            metadata = {
                "source_compressed_sha256": "source",
                "guest_script_sha256": prepare.SCRIPT_SHA256,
                "guest_script_path": prepare.GUEST_SCRIPT_PATH,
                "raw_size": len(b"wateros"),
                "prepared_raw_sha256": hashlib.sha256(b"wateros").hexdigest(),
                "e2fsck_exit_code": 0,
            }
            prepare.manifest_path(image).write_text(json.dumps(metadata))
            original_size = prepare.RAW_IMAGE_SIZE
            prepare.RAW_IMAGE_SIZE = len(b"wateros")
            try:
                self.assertTrue(prepare.existing_master_is_reusable(image, "source"))
                image.write_bytes(b"changed")
                self.assertFalse(prepare.existing_master_is_reusable(image, "source"))
            finally:
                prepare.RAW_IMAGE_SIZE = original_size

    def test_sparse_decompression_preserves_content_and_size(self) -> None:
        payload = (
            b"head"
            + bytes(prepare.SPARSE_BLOCK_SIZE - 4)
            + bytes(8 * 1024 * 1024)
            + b"tail"
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "input.gz"
            destination = Path(directory) / "output.img"
            with gzip.open(source, "wb") as stream:
                stream.write(payload)
            size = prepare.decompress_sparse(source, destination)
            self.assertEqual(size, len(payload))
            self.assertEqual(destination.read_bytes(), payload)
            self.assertLess(destination.stat().st_blocks * 512, len(payload) // 4)


if __name__ == "__main__":
    unittest.main()
