from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS / "perf"))

import buildstorm_runner as runner


def successful_log(elapsed: str = "499.25") -> str:
    cagent = "\n".join(f"testcase cagent case{i} pass 1" for i in range(10))
    return "\n".join((
        "#### OS COMP TEST GROUP START cagent-glibc ####",
        cagent,
        "#### OS COMP TEST GROUP END cagent-glibc ####",
        "TOOLCHAIN_RESULT status=OK",
        "MINIBUILD_RESULT status=OK",
        "BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=8 "
        f"elapsed_s={elapsed} artifact=/tmp/a bytes=700000 run=OK",
    ))


class BuildStormRunnerTests(unittest.TestCase):
    def test_riscv_command_matches_online_qemu_contract(self) -> None:
        command = runner.qemu_argv("rv", Path("/kernel"), Path("/image"), Path("/qemu"))
        self.assertEqual(command[0], "/qemu/qemu-system-riscv64")
        self.assertIn("16G", command)
        self.assertIn("8", command)
        self.assertIn("virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0", command)
        self.assertNotIn("-snapshot", command)

    def test_loongarch_command_matches_online_qemu_contract(self) -> None:
        command = runner.qemu_argv("la", Path("/kernel"), Path("/image"), Path("/qemu"))
        self.assertEqual(command[0], "/qemu/qemu-system-loongarch64")
        self.assertIn("36G", command)
        self.assertIn("12", command)
        self.assertIn("virtio-blk-pci,drive=x0", command)
        self.assertNotIn("-snapshot", command)

    def test_result_parser_requires_complete_protocol(self) -> None:
        summary = runner.summarize_log(successful_log(), timed_out=False)
        self.assertEqual(summary["status"], "passed")
        self.assertEqual(summary["cagent_passes"], 10)
        self.assertEqual(summary["guest_elapsed_s"], 499.25)
        self.assertEqual(summary["performance_decision"], "retain_without_performance_rerun")

    def test_result_parser_rejects_panic_before_result(self) -> None:
        summary = runner.summarize_log("kernel panic\n" + successful_log(), timed_out=False)
        self.assertEqual(summary["status"], "failed")
        self.assertTrue(summary["fatal_before_result"])

    def test_result_parser_rejects_panic_between_old_and_final_result(self) -> None:
        log = "\n".join((
            "BUILDSTORM_RESULT mode=multi status=FAIL rc=1 elapsed_s=1 run=FAIL",
            "kernel panic",
            successful_log(),
        ))
        summary = runner.summarize_log(log, timed_out=False)
        self.assertEqual(summary["status"], "failed")
        self.assertTrue(summary["fatal_before_result"])

    def test_performance_thresholds(self) -> None:
        self.assertEqual(runner.performance_decision(499.99), "retain_without_performance_rerun")
        self.assertEqual(runner.performance_decision(500.0), "rerun_required")
        self.assertEqual(runner.performance_decision(519.99), "rerun_required")
        self.assertEqual(runner.performance_decision(520.0), "reject_not_faster_than_baseline")

    def test_parse_last_result_wins(self) -> None:
        log = "\n".join((
            "BUILDSTORM_RESULT mode=multi status=FAIL rc=1 elapsed_s=1 run=FAIL",
            "BUILDSTORM_RESULT mode=multi status=OK rc=0 elapsed_s=499 run=OK",
        ))
        self.assertEqual(runner.parse_last_buildstorm_result(log)["status"], "OK")

    def test_image_cache_evict_syncs_and_advises_only_selected_file(self) -> None:
        with tempfile.NamedTemporaryFile() as image, patch.object(runner.os, "sync") as sync, patch.object(
            runner.os, "posix_fadvise"
        ) as advise:
            runner.evict_image_cache(Path(image.name))
        sync.assert_called_once_with()
        advise.assert_called_once()
        self.assertEqual(advise.call_args.args[1:], (0, 0, runner.os.POSIX_FADV_DONTNEED))

    def test_execute_stops_after_success_marker(self) -> None:
        program = (
            "import time; "
            "print('BUILDSTORM_RESULT mode=multi status=OK rc=0 elapsed_s=1 run=OK', flush=True); "
            "time.sleep(10)"
        )
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "serial.log"
            returncode, timed_out, stopped, wall, _ = runner.execute(
                [sys.executable, "-c", program], log, 2
            )
        self.assertFalse(timed_out)
        self.assertTrue(stopped)
        self.assertLess(wall, 2)
        self.assertIsNotNone(returncode)


if __name__ == "__main__":
    unittest.main()
