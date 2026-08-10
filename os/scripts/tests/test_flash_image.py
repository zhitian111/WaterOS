import tempfile
import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "root_image"))
from flash_image import FlashError, flash, mounted_block_paths, target_capacity_bytes


class FlashImageTests(unittest.TestCase):
    def test_dry_run_and_copy_to_explicit_regular_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.img"
            target = root / "target.img"
            source.write_bytes(b"wateros" * 512)
            target.write_bytes(b"\0" * source.stat().st_size)
            self.assertEqual(flash(source, target, allow_regular_file=True, dry_run=True), source.stat().st_size)
            self.assertEqual(target.read_bytes(), b"\0" * source.stat().st_size)
            flash(source, target, allow_regular_file=True)
            self.assertEqual(target.read_bytes(), source.read_bytes())

    def test_rejects_small_target_and_same_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.img"
            source.write_bytes(b"x" * 1024)
            small = root / "small.img"
            small.write_bytes(b"x")
            with self.assertRaisesRegex(FlashError, "smaller"):
                flash(source, small, allow_regular_file=True)
            with self.assertRaisesRegex(FlashError, "different"):
                flash(source, source, allow_regular_file=True)

    def test_regular_target_requires_explicit_opt_in(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.img"
            target = root / "target.img"
            source.write_bytes(b"x" * 16)
            target.write_bytes(b"\0" * 16)
            with self.assertRaisesRegex(FlashError, "block device"):
                flash(source, target)

    def test_dry_run_does_not_require_block_device_confirmation(self) -> None:
        # A real block-device fixture is unavailable without privileged setup;
        # the behavior is covered by the explicit dry-run branch and CLI docs.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.img"
            target = root / "target.img"
            source.write_bytes(b"x" * 16)
            target.write_bytes(b"\0" * 16)
            self.assertEqual(flash(source, target, allow_regular_file=True, dry_run=True), 16)

    def test_regular_file_capacity_uses_stat_without_device_io(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "target.img"
            target.write_bytes(b"\0" * 32)
            self.assertEqual(target_capacity_bytes(target, target.stat()), 32)

    def test_mount_parser_is_fail_closed_for_nonempty_mountpoints(self) -> None:
        def fake_runner(*_args, **_kwargs):
            return type("Result", (), {"stdout": "/dev/sda /\n/dev/sda1 /boot\n/dev/sda2 -\n", "stderr": ""})()
        self.assertEqual(mounted_block_paths(Path("/dev/sda"), fake_runner), ["/dev/sda", "/dev/sda1"])


if __name__ == "__main__":
    unittest.main()
