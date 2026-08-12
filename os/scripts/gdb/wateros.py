"""面向导出 WATEROS_DEBUG_STATE 的 WaterOS 内核注册 GDB 命令。"""
from __future__ import annotations

import json
import struct
import sys
from pathlib import Path

import gdb
import gdb.unwinder

_DEBUG_DIR = Path(__file__).resolve().parents[1] / "debug"
if str(_DEBUG_DIR) not in sys.path:
    sys.path.insert(0, str(_DEBUG_DIR))

from debug_abi import (  # noqa: E402
    HEADER_SIZE,
    decode_state,
    lock_wait_edges,
    parse_layout,
    render_cpus,
    render_events,
)


class WaterOsSwitchUnwinder(gdb.unwinder.Unwinder):
    """CPU 停在 __switch 内部时恢复即将切入任务的栈帧。

    普通 CFA 无法描述 SP 和 RA 开始指向另一任务的瞬间。两种架构的 TaskContext
    都刻意以 `ra, sp, fp` 开头，因此一个小型 unwinder 即可覆盖 RISC-V 和 LoongArch。
    """

    def __init__(self) -> None:
        super().__init__("wateros-switch")

    def __call__(self, pending_frame):
        try:
            pc = int(pending_frame.read_register("pc"))
            symbol = gdb.execute(f"info symbol 0x{pc:x}", to_string=True).strip()
            if not (symbol.startswith("__switch ") or symbol.startswith("__switch+")):
                return None
            architecture = pending_frame.architecture().name().lower()
            if "loongarch" in architecture:
                context_register, sp_register, fp_register, ra_register = (
                    "r5",
                    "r3",
                    "r22",
                    "r1",
                )
            else:
                context_register, sp_register, fp_register, ra_register = (
                    "a1",
                    "sp",
                    "s0",
                    "ra",
                )
            context = int(pending_frame.read_register(context_register))
            raw = bytes(gdb.selected_inferior().read_memory(context, 24))
            ra, sp, fp = struct.unpack_from("<QQQ", raw)
            frame_id = gdb.unwinder.FrameId(gdb.Value(sp), gdb.Value(ra))
            unwind = pending_frame.create_unwind_info(frame_id)
            unwind.add_saved_register("pc", gdb.Value(ra))
            unwind.add_saved_register(ra_register, gdb.Value(ra))
            unwind.add_saved_register(sp_register, gdb.Value(sp))
            unwind.add_saved_register(fp_register, gdb.Value(fp))
            return unwind
        except (gdb.error, gdb.MemoryError, ValueError, struct.error):
            return None


def _symbol_address(name: str) -> int:
    try:
        return int(gdb.parse_and_eval(f"&{name}"))
    except gdb.error as exc:
        raise gdb.GdbError(
            f"ELF/guest has no {name}; use a matching *-gdb kernel: {exc}"
        ) from exc


def _local_elf_bytes(virtual_address: int, size: int) -> bytes:
    filename = gdb.current_progspace().filename
    if not filename:
        raise gdb.GdbError("no local WaterOS ELF is loaded")
    data = Path(filename).read_bytes()
    if data[:4] != b"\x7fELF" or data[4] != 2:
        raise gdb.GdbError(f"local symbol file is not ELF64: {filename}")
    endian = "<" if data[5] == 1 else ">"
    phoff = struct.unpack_from(endian + "Q", data, 32)[0]
    phentsize = struct.unpack_from(endian + "H", data, 54)[0]
    phnum = struct.unpack_from(endian + "H", data, 56)[0]
    for index in range(phnum):
        offset = phoff + index * phentsize
        p_type = struct.unpack_from(endian + "I", data, offset)[0]
        if p_type != 1:
            continue
        file_offset, vaddr, _paddr, file_size, _mem_size = struct.unpack_from(
            endian + "QQQQQ", data, offset + 8
        )
        if vaddr <= virtual_address and virtual_address + size <= vaddr + file_size:
            start = file_offset + virtual_address - vaddr
            return data[start : start + size]
    raise gdb.GdbError(f"local ELF does not back address 0x{virtual_address:x}")


def read_snapshot(event_limit: int = 64) -> dict:
    inferior = gdb.selected_inferior()
    response = gdb.execute("maintenance packet Qqemu.PhyMemMode:1", to_string=True)
    if "OK" not in response:
        raise gdb.GdbError(f"QEMU 物理内存模式启用失败：{response.strip()}")
    try:
        address = _symbol_address("WATEROS_DEBUG_STATE")
        header = bytes(inferior.read_memory(address, HEADER_SIZE))
        layout = parse_layout(header)
        raw = bytes(inferior.read_memory(address, layout.total_size))
        snapshot = decode_state(raw, event_limit=event_limit)
        snapshot["observed_vcpus"] = len(inferior.threads())
        build_address = _symbol_address("WATEROS_DEBUG_BUILD_ID")
        build_raw = bytes(inferior.read_memory(build_address, layout.build_id_size))
    finally:
        gdb.execute("maintenance packet Qqemu.PhyMemMode:0", to_string=True)
    snapshot["build_id"] = build_raw.split(b"\0", 1)[0].decode("ascii", "replace")
    local_raw = _local_elf_bytes(build_address, layout.build_id_size)
    local_build = local_raw.split(b"\0", 1)[0].decode("ascii", "replace")
    if snapshot["build_id"] != local_build:
        raise gdb.GdbError(
            f"ELF/guest build ID mismatch: elf={local_build!r} "
            f"guest={snapshot['build_id']!r}"
        )
    if snapshot["build_id"] != layout.build_id:
        raise gdb.GdbError(
            f"debug header/build symbol mismatch: header={layout.build_id!r} "
            f"symbol={snapshot['build_id']!r}"
        )
    return snapshot


class WaterOsCpus(gdb.Command):
    def __init__(self) -> None:
        super().__init__("wos-cpus", gdb.COMMAND_STATUS)

    def invoke(self, argument: str, from_tty: bool) -> None:
        del argument, from_tty
        print(render_cpus(read_snapshot()))


class WaterOsEvents(gdb.Command):
    def __init__(self) -> None:
        super().__init__("wos-events", gdb.COMMAND_STATUS)

    def invoke(self, argument: str, from_tty: bool) -> None:
        del from_tty
        cpu = int(argument, 0) if argument.strip() else None
        print(render_events(read_snapshot(), cpu))


class WaterOsLocks(gdb.Command):
    def __init__(self) -> None:
        super().__init__("wos-locks", gdb.COMMAND_STATUS)

    def invoke(self, argument: str, from_tty: bool) -> None:
        del argument, from_tty
        snapshot = read_snapshot()
        edges = lock_wait_edges(snapshot)
        if not edges:
            print("<no tracked lock waiters>")
            return
        for edge in edges:
            lock = edge["lock"]
            owner = edge["owner_cpu"]
            print(
                f"cpu={edge['waiter_cpu']} waits {lock['name']}@0x{lock['object']:x} "
                f"owner_cpu={owner if owner is not None else '?'}"
            )


class WaterOsTasks(gdb.Command):
    def __init__(self) -> None:
        super().__init__("wos-tasks", gdb.COMMAND_STATUS)

    def invoke(self, argument: str, from_tty: bool) -> None:
        del argument, from_tty
        snapshot = read_snapshot()
        active = [cpu for cpu in snapshot["cpus"] if cpu["current_task"] is not None]
        for cpu in active:
            print(
                f"task={cpu['current_task']} cpu={cpu['cpu']} "
                f"mode={'USER' if cpu['user'] else ('IDLE' if cpu['idle'] else 'KERN')} "
                f"kind={cpu['task_kind_name']} state={cpu['task_state_name']} "
                f"policy={cpu['sched_policy']} nice={cpu['nice']} "
                f"wait={cpu['wait_kind_name']}:{cpu['wait_value']} "
                f"last_schedule={cpu['last_schedule_reason_name']} "
                f"aspace=0x{cpu['address_space']:x} trap_pc=0x{cpu['last_trap_pc']:x} "
                f"trap_sp=0x{cpu['last_trap_sp']:x} syscall={cpu['last_syscall_nr']}"
            )


class WaterOsTask(gdb.Command):
    def __init__(self) -> None:
        super().__init__("wos-task", gdb.COMMAND_STATUS)

    def invoke(self, argument: str, from_tty: bool) -> None:
        del from_tty
        if not argument.strip():
            raise gdb.GdbError("用法：wos-task <task-id>")
        task = int(argument, 0)
        snapshot = read_snapshot(event_limit=256)
        current = [cpu for cpu in snapshot["cpus"] if cpu["current_task"] == task]
        events = [event for event in snapshot["events"] if event["task"] == task]
        print(json.dumps({"task": task, "current": current, "events": events}, indent=2))


class WaterOsSnapshot(gdb.Command):
    def __init__(self) -> None:
        super().__init__("wos-snapshot", gdb.COMMAND_STATUS)

    def invoke(self, argument: str, from_tty: bool) -> None:
        del from_tty
        event_limit = int(argument, 0) if argument.strip() else 64
        snapshot = read_snapshot(event_limit=event_limit)
        print(f"WaterOS debug ABI={snapshot['abi_version']} build={snapshot['build_id']}")
        print(render_cpus(snapshot))
        print("\nRecent events:")
        print(render_events(snapshot))
        print("\nLock wait edges:")
        edges = lock_wait_edges(snapshot)
        print(json.dumps(edges, indent=2) if edges else "<none>")


WaterOsCpus()
WaterOsEvents()
WaterOsLocks()
WaterOsTasks()
WaterOsTask()
WaterOsSnapshot()
gdb.unwinder.register_unwinder(None, WaterOsSwitchUnwinder(), replace=True)
print("WaterOS GDB 命令已加载：wos-cpus/tasks/task/events/locks/snapshot")
