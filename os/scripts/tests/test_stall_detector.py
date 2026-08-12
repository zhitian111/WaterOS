"""验证重复 PC、任务停滞和等待链的判定逻辑。"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

DEBUG_SCRIPTS = Path(__file__).resolve().parents[1] / "debug"
sys.path.insert(0, str(DEBUG_SCRIPTS))

from wateros_debug import (
    RemoteSample,
    classify_stall,
    is_quiescent,
    stagnation_reasons,
)


def cpu(
    cpu_id: int,
    *,
    timer: int = 10,
    runnable: list[int] | None = None,
    need_resched: bool = False,
    idle: bool = False,
    waiting: tuple[int, int] = (0, 0),
    held: list[tuple[int, int]] | None = None,
    reason: int = 0,
) -> dict:
    return {
        "cpu": cpu_id,
        "online": True,
        "idle": idle,
        "current_task": cpu_id,
        "timer_ticks": timer,
        "context_switches": 2,
        "syscalls": 3,
        "traps": timer,
        "ipi_sent": 0,
        "ipi_received": 0,
        "last_syscall_nr": 0,
        "last_syscall_pc": 0x1000,
        "last_trap_pc": 0x2000,
        "runnable": runnable or [0, 0, 0, 0, 0],
        "need_resched": need_resched,
        "last_schedule_reason": reason,
        "waiting_lock": {"kind": waiting[0], "object": waiting[1]},
        "held_locks": [
            {"kind": kind, "object": address} for kind, address in (held or [])
        ],
    }


def sample(cpus: list[dict], *, pc: int = 0x8000, events: list[dict] | None = None) -> RemoteSample:
    return RemoteSample(
        "T05",
        [
            {
                "cpu": item["cpu"],
                "thread": str(item["cpu"] + 1),
                "pc": pc,
                "sp": 0x9000,
                "ra": 0,
                "fp": 0,
            }
            for item in cpus
        ] or [{"cpu": 0, "thread": "1", "pc": pc, "sp": 0x9000, "ra": 0, "fp": 0}],
        {
            "cpus": cpus,
            "events": events or [],
            "event_meta": [
                {"cpu": item["cpu"], "next_sequence": item["timer_ticks"]}
                for item in cpus
            ],
        },
        "test-build",
    )


class StallDetectorTests(unittest.TestCase):
    def test_idle_empty_system_is_quiescent(self) -> None:
        self.assertTrue(is_quiescent(sample([cpu(0, idle=True)])))

    def test_abba_lock_cycle_wins_classification(self) -> None:
        current = sample(
            [
                cpu(0, waiting=(2, 0xB), held=[(1, 0xA)]),
                cpu(1, waiting=(1, 0xA), held=[(2, 0xB)]),
            ]
        )
        self.assertEqual(classify_stall(current), "lock-deadlock")

    def test_runnable_need_resched_is_scheduler_starvation(self) -> None:
        current = sample([cpu(0, runnable=[1, 0, 0, 0, 0], need_resched=True)])
        self.assertEqual(classify_stall(current), "scheduler-starvation")

    def test_unchanged_timer_is_interrupt_stall(self) -> None:
        baseline = sample([cpu(0, timer=22)])
        current = sample([cpu(0, timer=22)])
        self.assertEqual(
            classify_stall(current, baseline), "interrupt-or-timer-stall"
        )

    def test_fixed_pc_loop_fallback(self) -> None:
        self.assertEqual(classify_stall(sample([])), "fixed-pc-loop")

    def test_recent_tlb_wait_is_identified(self) -> None:
        current = sample([cpu(0)], events=[{"name": "tlb-shootdown"}])
        self.assertEqual(classify_stall(current), "tlb-shootdown-wait")

    def test_unanswered_ipi_is_identified(self) -> None:
        current = sample([cpu(0)], events=[{"name": "ipi-send"}])
        self.assertEqual(classify_stall(current), "ipi-delivery-wait")

    def test_fault_marker_selects_deterministic_reason(self) -> None:
        current = sample([cpu(0, reason=0xF0170004)])
        self.assertEqual(classify_stall(current), "scheduler-starvation")

    def test_one_stuck_cpu_is_visible_while_another_cpu_advances(self) -> None:
        previous = sample([cpu(0, timer=10), cpu(1, timer=10, idle=True)])
        current = sample([cpu(0, timer=10), cpu(1, timer=11, idle=True)])
        reasons = stagnation_reasons(previous, current)
        self.assertIn("cpu0:timer", reasons)
        self.assertIn("cpu0:fixed", reasons)
        self.assertFalse(any(reason.startswith("cpu1:") for reason in reasons))

    def test_user_compute_progress_is_not_stagnant(self) -> None:
        previous = sample([cpu(0, timer=10)], pc=0x8000)
        current = sample([cpu(0, timer=11)], pc=0x8010)
        self.assertEqual(stagnation_reasons(previous, current), set())

    def test_scheduler_starvation_ignores_timer_progress(self) -> None:
        previous = sample(
            [cpu(0, timer=10, runnable=[1, 0, 0, 0, 0], need_resched=True)]
        )
        current = sample(
            [cpu(0, timer=11, runnable=[1, 0, 0, 0, 0], need_resched=True)]
        )
        self.assertIn("cpu0:scheduler", stagnation_reasons(previous, current))


if __name__ == "__main__":
    unittest.main()
