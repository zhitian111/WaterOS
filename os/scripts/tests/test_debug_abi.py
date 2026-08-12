"""验证内核调试 ABI 的布局检查、解析与等待关系提取。"""

from __future__ import annotations

import struct
import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
DEBUG = SCRIPTS / "debug"
sys.path[:0] = [str(SCRIPTS), str(DEBUG)]

from debug_abi import (
    DEBUG_MAGIC,
    HEADER_SIZE,
    decode_state,
    lock_wait_edges,
    parse_layout,
)


def fixture() -> bytearray:
    max_cpus, capacity, cpu_size, event_size = 2, 4, 328, 56
    build_id = b"test-build".ljust(64, b"\0")
    header = struct.pack(
        "<QIHHIIIHH", DEBUG_MAGIC, 1, max_cpus, capacity, cpu_size, event_size, 64, 1, 0
    ) + build_id
    cpu_slots_size = 24 + 2 * cpu_size
    event_slot_size = 8 + event_size
    cpu_events_size = 16 + capacity * event_slot_size
    data = bytearray(header)
    data.extend(b"\0" * (max_cpus * cpu_slots_size + max_cpus * cpu_events_size))

    cpu0 = HEADER_SIZE + 24
    values = [1, 1, 42, 0x1234, 10, 3, 2, 4, 1, 1, 8, 0x80200000, 0x9000, 0, 98, 0x1000]
    struct.pack_into("<16Q", data, cpu0, *values)
    struct.pack_into("<5I", data, cpu0 + 128, 1, 0, 0, 0, 0)
    struct.pack_into("<4H", data, cpu0 + 152, 1, 2, 0, 0)
    # CPU 0 holds scheduler.
    struct.pack_into("<I", data, cpu0 + 192, 1)
    struct.pack_into("<H", data, cpu0 + 200, 1)
    struct.pack_into("<Q", data, cpu0 + 208, 0xDEAD)

    cpu1_base = HEADER_SIZE + cpu_slots_size
    cpu1 = cpu1_base + 24
    values[1], values[2] = 1, 43
    struct.pack_into("<16Q", data, cpu1, *values)
    # CPU 1 waits on the same scheduler lock.
    struct.pack_into("<H", data, cpu1 + 176, 1)
    struct.pack_into("<Q", data, cpu1 + 184, 0xDEAD)

    events_base = HEADER_SIZE + max_cpus * cpu_slots_size
    struct.pack_into("<Q", data, events_base, 2)  # next sequence
    slot = events_base + 16 + event_slot_size  # sequence 1 -> index 1
    struct.pack_into("<Q", data, slot, 1)
    struct.pack_into("<QQHHIQQQQ", data, slot + 8, 10, 42, 2, 0, 0, 0, 1, 2, 3)
    return data


class DebugAbiTests(unittest.TestCase):
    def test_layout_and_cpu_state(self) -> None:
        data = fixture()
        layout = parse_layout(data[:HEADER_SIZE])
        self.assertEqual(layout.total_size, len(data))
        self.assertEqual(layout.arch, 1)
        self.assertEqual(layout.build_id, "test-build")
        snapshot = decode_state(data)
        self.assertEqual(snapshot["cpus"][0]["current_task"], 42)
        self.assertEqual(snapshot["cpus"][0]["runnable"], [1, 0, 0, 0, 0])
        self.assertEqual(snapshot["cpus"][0]["task_state_name"], "running")
        self.assertEqual(snapshot["events"][0]["name"], "task-switch")

    def test_lock_owner_edge(self) -> None:
        edges = lock_wait_edges(decode_state(fixture()))
        self.assertEqual(edges[0]["waiter_cpu"], 1)
        self.assertEqual(edges[0]["owner_cpu"], 0)

    def test_incomplete_event_is_ignored(self) -> None:
        data = fixture()
        layout = parse_layout(data[:HEADER_SIZE])
        events_base = HEADER_SIZE + layout.max_cpus * layout.cpu_slots_size
        slot = events_base + 16 + layout.event_slot_size
        struct.pack_into("<Q", data, slot, 99)
        self.assertEqual(decode_state(data)["events"], [])

    def test_event_ring_wrap_keeps_latest_sequences(self) -> None:
        data = fixture()
        layout = parse_layout(data[:HEADER_SIZE])
        events_base = HEADER_SIZE + layout.max_cpus * layout.cpu_slots_size
        struct.pack_into("<Q", data, events_base, 7)
        for sequence in range(3, 7):
            slot = (
                events_base
                + 16
                + (sequence % layout.event_capacity) * layout.event_slot_size
            )
            struct.pack_into("<Q", data, slot, sequence)
            struct.pack_into(
                "<QQHHIQQQQ",
                data,
                slot + 8,
                sequence,
                42,
                2,
                0,
                0,
                0,
                0,
                0,
                0,
            )
        sequences = [event["sequence"] for event in decode_state(data)["events"]]
        self.assertEqual(sequences, [3, 4, 5, 6])


if __name__ == "__main__":
    unittest.main()
