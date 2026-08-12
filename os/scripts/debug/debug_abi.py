#!/usr/bin/env python3
"""解析稳定且无需动态分配的 ``WATEROS_DEBUG_STATE`` ABI。"""
from __future__ import annotations

import struct
from dataclasses import dataclass
from typing import Any

DEBUG_MAGIC = 0x5741544552444247
HEADER_SIZE = 96
CPU_SLOTS_PREFIX = 24
EVENT_SLOT_PREFIX = 8
CPU_EVENTS_PREFIX = 16
LOCK_SIZE = 16
HELD_LOCKS = 8

EVENT_NAMES = {
    1: "task-enqueue",
    2: "task-switch",
    3: "task-block",
    4: "task-wake",
    5: "task-exit",
    6: "syscall-enter",
    7: "syscall-exit",
    8: "trap-enter",
    9: "trap-exit",
    10: "timer",
    11: "ipi-send",
    12: "ipi-receive",
    13: "futex-wait",
    14: "futex-wake",
    15: "tlb-shootdown",
    16: "lock-contended",
    17: "lock-acquire",
    18: "lock-release",
}

LOCK_NAMES = {
    0: "none",
    1: "scheduler",
    2: "process-registry",
    3: "futex-registry",
    4: "frame-allocator",
    5: "address-space",
    6: "vfs",
    7: "network",
    8: "klog",
}

TASK_KIND_NAMES = {0: "none", 1: "kernel", 2: "user"}
TASK_STATE_NAMES = {
    0: "none",
    1: "ready",
    2: "running",
    3: "blocking",
    4: "sleeping",
    5: "exited",
}
WAIT_KIND_NAMES = {
    0: "none",
    1: "waitqueue",
    2: "task-exit",
    3: "child-exit",
    4: "manual",
    5: "sleep-until",
    6: "exit-code",
}
SCHEDULE_REASON_NAMES = {
    0: "none",
    1: "start-first",
    2: "yield",
    3: "reschedule",
    4: "tick",
    5: "block",
    6: "sleep",
    7: "exit",
}


class DebugAbiError(RuntimeError):
    pass


@dataclass(frozen=True)
class DebugLayout:
    version: int
    arch: int
    build_id: str
    max_cpus: int
    event_capacity: int
    cpu_state_size: int
    event_size: int
    build_id_size: int

    @property
    def cpu_slots_size(self) -> int:
        return CPU_SLOTS_PREFIX + 2 * self.cpu_state_size

    @property
    def event_slot_size(self) -> int:
        return EVENT_SLOT_PREFIX + self.event_size

    @property
    def cpu_events_size(self) -> int:
        return CPU_EVENTS_PREFIX + self.event_capacity * self.event_slot_size

    @property
    def total_size(self) -> int:
        return (
            HEADER_SIZE
            + self.max_cpus * self.cpu_slots_size
            + self.max_cpus * self.cpu_events_size
        )


def parse_layout(header: bytes) -> DebugLayout:
    if len(header) < HEADER_SIZE:
        raise DebugAbiError("debug header is truncated")
    magic, version, max_cpus, event_capacity, cpu_size, event_size, build_id_size, arch, _ = (
        struct.unpack_from("<QIHHIIIHH", header)
    )
    if magic != DEBUG_MAGIC:
        raise DebugAbiError(f"bad debug magic: 0x{magic:016x}")
    if version != 1:
        raise DebugAbiError(f"unsupported debug ABI version: {version}")
    if arch not in (1, 2):
        raise DebugAbiError(f"unsupported debug architecture: {arch}")
    if not 1 <= max_cpus <= 256:
        raise DebugAbiError(f"invalid CPU count: {max_cpus}")
    if not 1 <= event_capacity <= 4096:
        raise DebugAbiError(f"invalid event capacity: {event_capacity}")
    if cpu_size < 328 or event_size < 56:
        raise DebugAbiError(
            f"invalid record sizes: cpu={cpu_size} event={event_size}"
        )
    if build_id_size != 64:
        raise DebugAbiError(f"unsupported build ID size: {build_id_size}")
    embedded_build_id = header[32 : 32 + build_id_size].split(b"\0", 1)[0]
    return DebugLayout(
        version,
        arch,
        embedded_build_id.decode("ascii", "replace"),
        max_cpus,
        event_capacity,
        cpu_size,
        event_size,
        build_id_size,
    )


def _u64(data: bytes, offset: int) -> int:
    return struct.unpack_from("<Q", data, offset)[0]


def _lock(data: bytes, offset: int) -> dict[str, Any]:
    kind = struct.unpack_from("<H", data, offset)[0]
    return {
        "kind": kind,
        "name": LOCK_NAMES.get(kind, f"lock-{kind}"),
        "object": _u64(data, offset + 8),
    }


def _cpu_state(data: bytes, offset: int, cpu: int) -> dict[str, Any]:
    values = struct.unpack_from("<16Q", data, offset)
    runnable = list(struct.unpack_from("<5I", data, offset + 128))
    last_reason = struct.unpack_from("<I", data, offset + 148)[0]
    task_kind, task_state, policy, wait_kind = struct.unpack_from("<4H", data, offset + 152)
    wait_value = _u64(data, offset + 160)
    nice = struct.unpack_from("<b", data, offset + 168)[0]
    waiting = _lock(data, offset + 176)
    held_count = min(struct.unpack_from("<I", data, offset + 192)[0], HELD_LOCKS)
    held = [_lock(data, offset + 200 + i * LOCK_SIZE) for i in range(held_count)]
    flags = values[1]
    task = values[2]
    return {
        "cpu": cpu,
        "generation": values[0],
        "flags": flags,
        "online": bool(flags & 1),
        "idle": bool(flags & 2),
        "user": bool(flags & 4),
        "need_resched": bool(flags & 8),
        "current_task": None if task == (1 << 64) - 1 else task,
        "address_space": values[3],
        "timer_ticks": values[4],
        "context_switches": values[5],
        "syscalls": values[6],
        "traps": values[7],
        "ipi_sent": values[8],
        "ipi_received": values[9],
        "last_trap_cause": values[10],
        "last_trap_pc": values[11],
        "last_trap_sp": values[12],
        "last_fault_addr": values[13],
        "last_syscall_nr": values[14],
        "last_syscall_pc": values[15],
        "runnable": runnable,
        "last_schedule_reason": last_reason,
        "last_schedule_reason_name": SCHEDULE_REASON_NAMES.get(
            last_reason, f"reason-0x{last_reason:x}"
        ),
        "task_kind": task_kind,
        "task_kind_name": TASK_KIND_NAMES.get(task_kind, f"kind-{task_kind}"),
        "task_state": task_state,
        "task_state_name": TASK_STATE_NAMES.get(task_state, f"state-{task_state}"),
        "sched_policy": policy,
        "nice": nice,
        "wait_kind": wait_kind,
        "wait_kind_name": WAIT_KIND_NAMES.get(wait_kind, f"wait-{wait_kind}"),
        "wait_value": wait_value,
        "waiting_lock": waiting,
        "held_locks": held,
    }


def _event(data: bytes, offset: int, sequence: int) -> dict[str, Any]:
    tick, task = struct.unpack_from("<QQ", data, offset)
    kind, cpu, flags = struct.unpack_from("<HHI", data, offset + 16)
    caller, arg0, arg1, arg2 = struct.unpack_from("<QQQQ", data, offset + 24)
    return {
        "sequence": sequence,
        "tick": tick,
        "task": None if task == (1 << 64) - 1 else task,
        "kind": kind,
        "name": EVENT_NAMES.get(kind, f"event-{kind}"),
        "cpu": cpu,
        "flags": flags,
        "caller_pc": caller,
        "args": [arg0, arg1, arg2],
    }


def decode_state(data: bytes, *, event_limit: int = 64) -> dict[str, Any]:
    layout = parse_layout(data[:HEADER_SIZE])
    if len(data) < layout.total_size:
        raise DebugAbiError(
            f"debug state truncated: need={layout.total_size} got={len(data)}"
        )
    cpus: list[dict[str, Any]] = []
    for cpu in range(layout.max_cpus):
        base = HEADER_SIZE + cpu * layout.cpu_slots_size
        published = _u64(data, base) & 1
        writing = bool(data[base + 8])
        dropped = _u64(data, base + 16)
        state_offset = base + CPU_SLOTS_PREFIX + published * layout.cpu_state_size
        state = _cpu_state(data, state_offset, cpu)
        state["writing"] = writing
        state["dropped_updates"] = dropped
        cpus.append(state)

    events_base = HEADER_SIZE + layout.max_cpus * layout.cpu_slots_size
    all_events: list[dict[str, Any]] = []
    event_meta: list[dict[str, int]] = []
    for cpu in range(layout.max_cpus):
        base = events_base + cpu * layout.cpu_events_size
        next_sequence = _u64(data, base)
        dropped = _u64(data, base + 8)
        first = max(1, next_sequence - min(event_limit, layout.event_capacity))
        for sequence in range(first, next_sequence):
            index = sequence % layout.event_capacity
            slot = base + CPU_EVENTS_PREFIX + index * layout.event_slot_size
            published_sequence = _u64(data, slot)
            if published_sequence != sequence:
                continue
            all_events.append(_event(data, slot + EVENT_SLOT_PREFIX, sequence))
        event_meta.append(
            {"cpu": cpu, "next_sequence": next_sequence, "dropped_events": dropped}
        )
    all_events.sort(key=lambda event: (event["tick"], event["cpu"], event["sequence"]))
    return {
        "abi_version": layout.version,
        "arch": layout.arch,
        "build_id": layout.build_id,
        "layout": {
            "max_cpus": layout.max_cpus,
            "event_capacity": layout.event_capacity,
            "cpu_state_size": layout.cpu_state_size,
            "event_size": layout.event_size,
            "total_size": layout.total_size,
        },
        "cpus": cpus,
        "event_meta": event_meta,
        "events": all_events,
    }


def render_cpus(snapshot: dict[str, Any]) -> str:
    event_meta = {item["cpu"]: item for item in snapshot.get("event_meta", [])}
    visible_cpus = snapshot["cpus"][: snapshot.get("observed_vcpus", len(snapshot["cpus"]))]
    lines = [
        "CPU ON MODE TASK RUNNABLE  TICKS SWITCH SYSCALL TRAP IPI(S/R) DROP(U/E) WAITING",
    ]
    for cpu in visible_cpus:
        mode = "OFF"
        if cpu["online"]:
            mode = "IDLE" if cpu["idle"] else ("USER" if cpu["user"] else "KERN")
        runnable = "/".join(str(value) for value in cpu["runnable"])
        task = "-" if cpu["current_task"] is None else str(cpu["current_task"])
        waiting = cpu["waiting_lock"]["name"] if cpu["waiting_lock"]["kind"] else "-"
        if cpu["wait_kind"]:
            waiting += f"/{cpu['wait_kind_name']}:{cpu['wait_value']}"
        dropped_events = event_meta.get(cpu["cpu"], {}).get("dropped_events", 0)
        dropped = f"{cpu['dropped_updates']}/{dropped_events}"
        if cpu["writing"]:
            dropped += "*"
        lines.append(
            f"{cpu['cpu']:>3} {'Y' if cpu['online'] else 'N':>2} {mode:<4} "
            f"{task:>8} {runnable:>10} {cpu['timer_ticks']:>6} "
            f"{cpu['context_switches']:>6} {cpu['syscalls']:>7} {cpu['traps']:>5} "
            f"{cpu['ipi_sent']:>4}/{cpu['ipi_received']:<4} {dropped:>9} {waiting}"
        )
    return "\n".join(lines)


def render_events(snapshot: dict[str, Any], cpu: int | None = None) -> str:
    events = snapshot["events"]
    if cpu is not None:
        events = [event for event in events if event["cpu"] == cpu]
    return "\n".join(
        f"tick={event['tick']:>8} cpu={event['cpu']} seq={event['sequence']:>6} "
        f"task={event['task']} {event['name']:<16} args={event['args']}"
        for event in events
    ) or "<no events>"


def lock_wait_edges(snapshot: dict[str, Any]) -> list[dict[str, Any]]:
    owners: dict[tuple[int, int], int] = {}
    for cpu in snapshot["cpus"]:
        for lock in cpu["held_locks"]:
            owners[(lock["kind"], lock["object"])] = cpu["cpu"]
    edges = []
    for cpu in snapshot["cpus"]:
        lock = cpu["waiting_lock"]
        if not lock["kind"]:
            continue
        edges.append(
            {
                "waiter_cpu": cpu["cpu"],
                "owner_cpu": owners.get((lock["kind"], lock["object"])),
                "lock": lock,
            }
        )
    return edges
