from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS / "perf"))

import buildstorm_runner as runner


class BuildStormRunnerTests(unittest.TestCase):
    def test_riscv_command_matches_measurement_protocol(self) -> None:
        command = runner.qemu_argv("rv", Path("/kernel"), Path("/image"))
        self.assertIn("16G", command)
        self.assertIn("8", command)
        self.assertIn("default", command)
        self.assertEqual(command.count("-snapshot"), 1)
        self.assertFalse(any("disk.img" in item for item in command))

    def test_loongarch_command_matches_measurement_protocol(self) -> None:
        command = runner.qemu_argv("la", Path("/kernel"), Path("/image"))
        self.assertIn("36G", command)
        self.assertIn("12", command)
        self.assertIn("virtio-blk-pci,drive=x0", command)
        self.assertEqual(command.count("-snapshot"), 1)

    def test_linux_userland_runs_official_script_as_init(self) -> None:
        command = runner.qemu_argv(
            "rv", Path("/kernel"), Path("/image"), linux_userland=True
        )
        self.assertIn("-append", command)
        bootargs = command[command.index("-append") + 1]
        self.assertIn("root=/dev/vda", bootargs)
        self.assertIn("init=/glibc/buildstorm_testcode.sh", bootargs)

    def test_parse_compile_uses_last_result(self) -> None:
        log = "\n".join((
            "BUILDSTORM_COMPILE mode=multi ok=false elapsed_s=1",
            "BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=12.5 cores=8",
        ))
        self.assertEqual(runner.parse_compile(log)["elapsed_s"], "12.5")

    def test_parse_perf_counters_uses_latest_snapshot_and_ignores_bad_values(self) -> None:
        log = "\n".join((
            "BUILDSTORM_PERF_COUNTERS page_hit=10 read_bytes=0x1000",
            "BUILDSTORM_PERF_COUNTERS page_hit=17 bad=unknown virtio_notify=3",
        ))
        self.assertEqual(
            runner.parse_perf_counters(log),
            {"page_hit": 17, "read_bytes": 4096, "virtio_notify": 3},
        )

    def test_image_cache_evict_syncs_and_advises_file(self) -> None:
        with tempfile.NamedTemporaryFile() as image, patch.object(runner.os, "sync") as sync, patch.object(
            runner.os, "posix_fadvise"
        ) as advise:
            runner.evict_image_cache(Path(image.name))
        sync.assert_called_once_with()
        advise.assert_called_once()
        self.assertEqual(advise.call_args.args[1:], (0, 0, runner.os.POSIX_FADV_DONTNEED))

    def test_execute_stops_after_complete_success_protocol(self) -> None:
        program = (
            "import time; "
            "print('BUILDSTORM_TOOLCHAIN ok', flush=True); "
            "print('BUILDSTORM_MINIBUILD ok', flush=True); "
            "print('BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1 cores=8', flush=True); "
            "time.sleep(10)"
        )
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "serial.log"
            returncode, timed_out, stopped, wall, _ = runner.execute(
                [sys.executable, "-c", program], log, 2
            )
            contents = log.read_text()
        self.assertFalse(timed_out)
        self.assertTrue(stopped)
        self.assertLess(wall, 2)
        self.assertIsNotNone(returncode)
        self.assertIn("BUILDSTORM_COMPILE", contents)


if __name__ == "__main__":
    unittest.main()
