from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools import image, root_image


REQUIRED_TOOLS = ("mke2fs", "debugfs", "e2fsck", "dumpe2fs")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def make_rootfs(root: Path) -> None:
    for directory in ("bin", "etc", "var/lib/wateros", "root", "tmp",
                      "var/tmp", "dev/shm"):
        (root / directory).mkdir(parents=True, exist_ok=True)
    (root / "root").chmod(0o700)
    for directory in ("tmp", "var/tmp", "dev/shm"):
        (root / directory).chmod(0o1777)
    busybox = root / "bin/busybox"
    busybox.write_bytes(b"synthetic busybox for image tests\n")
    busybox.chmod(0o755)
    (root / "bin/sh").symlink_to("busybox")
    (root / "etc/wateros-release").write_text("NAME=WaterOS\n", encoding="utf-8")
    (root / "var/lib/wateros/packages.json").write_text(
        json.dumps({"schema": 1, "packages": []}) + "\n", encoding="utf-8")


@unittest.skipUnless(all(shutil.which(tool) for tool in REQUIRED_TOOLS),
                     "e2fsprogs tools are required")
class Ext4IntegrationTests(unittest.TestCase):
    def debugfs(self, image_path: Path, command: str) -> str:
        result = subprocess.run(["debugfs", "-R", command, str(image_path)],
                                check=True, text=True, capture_output=True)
        return result.stdout + result.stderr

    def test_create_image_and_required_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            staging = root / "staging"
            staging.mkdir()
            make_rootfs(staging)
            output = root / "wateros.ext4"
            image.create_image(staging, output, "rv", 32, 4096, 256)
            self.assertTrue(output.is_file())
            self.assertTrue(output.with_suffix(".ext4.manifest.json").is_file())
            self.assertTrue(output.with_suffix(".ext4.sha256").is_file())
            link_stat = self.debugfs(output, "stat /bin/sh")
            self.assertIn("Fast link dest: \"busybox\"", link_stat)
            busybox_stat = self.debugfs(output, "stat /bin/busybox")
            self.assertRegex(busybox_stat, r"User:\s+0\s+Group:\s+0")
            features = subprocess.run(["dumpe2fs", "-h", str(output)], check=True,
                                      text=True, capture_output=True).stdout
            self.assertIn("64bit", next(line for line in features.splitlines()
                                         if line.startswith("Filesystem features:")))
            self.assertIn("Group descriptor size:    64", features)
            second = root / "wateros-second.ext4"
            image.create_image(staging, second, "rv", 32, 4096, 256)
            first_hash = sha256(output)
            second_hash = sha256(second)
            headers = "\n--- second image ---\n".join(
                subprocess.run(["dumpe2fs", "-h", str(candidate)], check=True,
                               text=True, capture_output=True).stdout
                for candidate in (output, second)
            )
            self.assertEqual(first_hash, second_hash, headers)

    def test_auto_image_size_scales_with_staging(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            staging = root / "staging"
            staging.mkdir()
            make_rootfs(staging)
            (staging / "large-file").write_bytes(b"x" * (20 * 1024 * 1024))
            size = image.auto_image_size_mb(staging, 4096, 256)
            self.assertGreaterEqual(size, 32)
            self.assertEqual(size & (size - 1), 0)

    def test_disk_image_appends_raw_filesystem_partition(self) -> None:
        if shutil.which("sfdisk") is None:
            self.skipTest("sfdisk is required")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            staging = root / "staging"
            staging.mkdir()
            make_rootfs(staging)
            extra = root / "extra.ext4"
            image.create_image(staging, extra, "rv", 16, 4096, 256)
            disk = root / "wateros.img"
            image.create_disk_image(staging, disk, "rv", 16, "gpt",
                                    extra_images=[extra], extra_partition_types=["L"])
            partitions = root_image.read_partitions(disk)
            self.assertEqual([partition.number for partition in partitions], [1, 2])
            self.assertEqual(partitions[0].start_sector, 2048)
            with disk.open("rb") as source:
                source.seek(partitions[1].byte_offset)
                self.assertEqual(source.read(extra.stat().st_size), extra.read_bytes())

    def test_mbr_rejects_more_than_four_partitions(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            images = []
            for index in range(4):
                path = root / f"extra-{index}.img"
                path.write_bytes(bytes([index + 1]))
                images.append(path)
            with self.assertRaisesRegex(root_image.ImageError, "at most four partitions"):
                root_image.make_partition_table_with_images(
                    root / "disk.img", 64 * 1024 * 1024, 16, images, [], "mbr"
                )

    def test_vf2_boot_layout_appends_filesystem_partition(self) -> None:
        if shutil.which("sfdisk") is None or shutil.which("mkfs.vfat") is None \
                or shutil.which("mcopy") is None:
            self.skipTest("sfdisk, mkfs.vfat and mcopy are required")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            staging = root / "staging"
            staging.mkdir()
            make_rootfs(staging)
            boot = root / "boot"
            boot.mkdir()
            (boot / "wateros.ui").write_bytes(b"synthetic ui\n")
            extra = root / "extra.ext4"
            image.create_image(staging, extra, "rv", 16, 4096, 256)
            disk = root / "wateros-vf2.img"
            image.create_disk_image(staging, disk, "rv", 64, "gpt",
                                    boot_dir=boot, boot_size_mb=8,
                                    extra_images=[extra], extra_partition_types=["L"])
            partitions = root_image.read_partitions(disk)
            self.assertEqual([partition.number for partition in partitions], [1, 2, 3, 4, 5])
            with disk.open("rb") as source:
                source.seek(partitions[4].byte_offset)
                self.assertEqual(source.read(extra.stat().st_size), extra.read_bytes())

    def test_overlay_keeps_base_and_writes_allowed_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            base_staging = root / "base-staging"
            base_staging.mkdir()
            make_rootfs(base_staging)
            base = root / "base.ext4"
            image.create_image(base_staging, base, "rv", 32, 4096, 256)
            original = sha256(base)

            overlay_staging = root / "overlay-staging"
            tool = overlay_staging / "opt/wateros/bin/hello"
            tool.parent.mkdir(parents=True)
            tool.write_text("#!/bin/sh\necho hello\n", encoding="utf-8")
            tool.chmod(0o755)
            (tool.parent / "hello-link").symlink_to("hello")
            spaced = tool.parent / "hello world"
            spaced.write_text("spaces are supported\n", encoding="utf-8")
            output = root / "overlay.ext4"
            image.create_overlay(overlay_staging, base, output, (), "rv")
            self.assertEqual(sha256(base), original)
            stat = self.debugfs(output, "stat /opt/wateros/bin/hello")
            self.assertIn("Type: regular", stat)
            self.assertIn("Mode:  0755", stat)
            self.assertRegex(stat, r"User:\s+0\s+Group:\s+0")
            link_stat = self.debugfs(output, "stat /opt/wateros/bin/hello-link")
            self.assertIn("Fast link dest: \"hello\"", link_stat)
            self.assertIn("Type: regular",
                          self.debugfs(output, 'stat "/opt/wateros/bin/hello world"'))
            changes = json.loads(output.with_suffix(".ext4.changes.json").read_text())
            self.assertEqual({entry["path"] for entry in changes["changes"]},
                             {"/opt", "/opt/wateros", "/opt/wateros/bin",
                              "/opt/wateros/bin/hello", "/opt/wateros/bin/hello-link",
                              "/opt/wateros/bin/hello world"})

            protected_staging = root / "protected-staging"
            protected = protected_staging / "glibc/do-not-touch"
            protected.parent.mkdir(parents=True)
            protected.write_text("forbidden", encoding="utf-8")
            rejected_output = root / "rejected.ext4"
            with self.assertRaisesRegex(image.ImageError, "protected path"):
                image.create_overlay(protected_staging, base, rejected_output, (), "rv")
            self.assertFalse(rejected_output.exists())
            self.assertEqual(sha256(base), original)

    def test_overlay_rejects_same_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            staging = root / "staging"
            staging.mkdir()
            base = root / "base.ext4"
            base.write_bytes(b"not used")
            with self.assertRaisesRegex(image.ImageError, "must differ"):
                image.create_overlay(staging, base, base, (), "rv")

    def test_default_overlay_name_does_not_duplicate_architecture(self) -> None:
        result = image.default_overlay_path(Path("sdcard-rv.img"), "rv")
        self.assertEqual(result.name, "sdcard-rv-wateros.ext4")

    def test_default_image_has_no_package_suffix(self) -> None:
        self.assertEqual(image.default_image_path("rv").name, "wateros-rv.ext4")


if __name__ == "__main__":
    unittest.main()
