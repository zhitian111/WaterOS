#!/usr/bin/env python3
"""WaterOS QEMU/GDB launcher, hang watcher and report collector."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from functools import lru_cache
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
OS_ROOT = SCRIPT_DIR.parent
DEBUG_DIR = SCRIPT_DIR / "debug"
GDB_EXTENSION = SCRIPT_DIR / "gdb" / "wateros.py"
if str(DEBUG_DIR) not in sys.path:
    sys.path.insert(0, str(DEBUG_DIR))
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from debug_abi import (  # noqa: E402
    HEADER_SIZE,
    DebugAbiError,
    decode_state,
    lock_wait_edges,
    parse_layout,
    render_cpus,
)
from gdb_remote_snapshot import (  # noqa: E402
    GdbRemote,
    RemoteError,
    find_register,
    read_memory,
    read_register,
    read_register_description,
    read_threads,
)


PROFILES = {
    "rv-pre": ("rv", "rv_pre_run-gdb", "kernel-rv-pre-gdb"),
    "rv-final": ("rv", "rv_final_run-gdb", "kernel-rv-final-gdb"),
    "rv-final-log": ("rv", "rv_final_run_log-gdb", "kernel-rv-final-log-gdb"),
    "rv-final-debug": ("rv", "rv_final_debug_run-gdb", "kernel-rv-final-debug-gdb"),
    "rv-smp-test": ("rv", "rv_final_smp_test-gdb", "kernel-rv-final-smp-test-gdb"),
    "la-pre": ("la", "la_pre_run-gdb", "kernel-la-pre-gdb"),
    "la-final": ("la", "la_final_run-gdb", "kernel-la-final-gdb"),
    "la-final-log": ("la", "la_final_run_log-gdb", "kernel-la-final-gdb"),
}

ARCH_TO_QEMU = {"rv": "qemu-system-riscv64", "la": "qemu-system-loongarch64"}
ARCH_TO_BINUTILS = {
    "rv": ["riscv64-unknown-elf-nm", "riscv64-elf-nm", "nm"],
    "la": ["loongarch64-linux-gnu-nm", "nm"],
}
ARCH_TO_ADDR2LINE = {
    "rv": ["riscv64-unknown-elf-addr2line", "riscv64-elf-addr2line", "addr2line"],
    "la": ["loongarch64-linux-gnu-addr2line", "addr2line"],
}
DEBUG_FAULT_REASON_BASE = 0xF0170000


class DebugToolError(RuntimeError):
    pass


def verify_build_id(expected: str, observed: str | None) -> None:
    """Reject symbolization unless the running guest matches the local ELF."""
    if observed != expected:
        raise DebugToolError(
            f"ELF/guest build ID mismatch: elf={expected!r} guest={observed!r}"
        )


def first_tool(candidates: list[str]) -> str | None:
    return next((tool for name in candidates if (tool := shutil.which(name))), None)


@lru_cache(maxsize=8)
def _nm_symbols(
    elf_name: str, arch: str, elf_size: int, elf_mtime_ns: int
) -> dict[str, tuple[int, int]]:
    del elf_size, elf_mtime_ns  # cache-key-only values invalidate a rebuilt ELF
    tool = first_tool(ARCH_TO_BINUTILS[arch])
    if tool is None:
        raise DebugToolError(f"no usable nm for architecture {arch}")
    try:
        output = subprocess.check_output(
            [tool, "-S", "--defined-only", elf_name], text=True, stderr=subprocess.PIPE
        )
    except subprocess.CalledProcessError as exc:
        raise DebugToolError(exc.stderr.strip() or f"{tool} failed") from exc
    symbols: dict[str, tuple[int, int]] = {}
    for line in output.splitlines():
        fields = line.split(None, 3)
        if len(fields) == 4:
            symbols[fields[3]] = (int(fields[0], 16), int(fields[1], 16))
        elif len(fields) == 3:
            # Hand-written assembly symbols frequently have no size column even
            # with `nm -S`: address, type, name.
            symbols[fields[2]] = (int(fields[0], 16), 0)
    return symbols


def nm_symbol(elf: Path, arch: str, name: str) -> tuple[int, int]:
    stat = elf.stat()
    symbols = _nm_symbols(str(elf.resolve()), arch, stat.st_size, stat.st_mtime_ns)
    if name in symbols:
        return symbols[name]
    raise DebugToolError(f"{elf} has no symbol {name}")


def read_elf_virtual(elf: Path, virtual_address: int, size: int) -> bytes:
    """Read bytes for a virtual address from an ELF64 PT_LOAD segment."""
    data = elf.read_bytes()
    if data[:4] != b"\x7fELF" or data[4] != 2:
        raise DebugToolError(f"not an ELF64 file: {elf}")
    endian = "<" if data[5] == 1 else ">"
    phoff = struct.unpack_from(endian + "Q", data, 32)[0]
    phentsize = struct.unpack_from(endian + "H", data, 54)[0]
    phnum = struct.unpack_from(endian + "H", data, 56)[0]
    for index in range(phnum):
        offset = phoff + index * phentsize
        p_type, _flags = struct.unpack_from(endian + "II", data, offset)
        if p_type != 1:
            continue
        file_offset, vaddr, _paddr, file_size, _mem_size = struct.unpack_from(
            endian + "QQQQQ", data, offset + 8
        )
        if vaddr <= virtual_address and virtual_address + size <= vaddr + file_size:
            start = file_offset + virtual_address - vaddr
            return data[start : start + size]
    raise DebugToolError(f"ELF address 0x{virtual_address:x} is not file-backed")


def local_build_id(elf: Path, arch: str) -> str:
    address, size = nm_symbol(elf, arch, "WATEROS_DEBUG_BUILD_ID")
    raw = read_elf_virtual(elf, address, min(size, 64))
    return raw.split(b"\0", 1)[0].decode("ascii", "replace")


def has_forced_frame_pointers(elf: Path, arch: str) -> bool:
    address, size = nm_symbol(elf, arch, "WATEROS_DEBUG_FRAME_POINTERS")
    return size == 1 and read_elf_virtual(elf, address, 1) == b"\x01"


def missing_cfi_symbols(elf: Path, arch: str) -> list[str]:
    """Return architecture boundary symbols not covered by any emitted FDE."""
    output = subprocess.check_output(
        ["readelf", "--debug-dump=frames", str(elf)], text=True, stderr=subprocess.PIPE
    )
    ranges = [
        (int(start, 16), int(end, 16))
        for start, end in re.findall(r"pc=([0-9a-fA-F]+)\.\.([0-9a-fA-F]+)", output)
    ]
    required = ("__alltraps", "__switch", "__arch_task_entry", "__arch_user_task_entry")
    missing = []
    for name in required:
        address, _size = nm_symbol(elf, arch, name)
        if not any(start <= address < end for start, end in ranges):
            missing.append(name)
    return missing


@lru_cache(maxsize=4096)
def symbolize_address(elf_name: str, arch: str, address: int) -> str:
    elf = Path(elf_name)
    stat = elf.stat()
    symbols = _nm_symbols(str(elf.resolve()), arch, stat.st_size, stat.st_mtime_ns)
    containing = [
        (start, size, name)
        for name, (start, size) in symbols.items()
        if start <= address < start + max(size, 1)
    ]
    symbol_text = "?"
    if containing:
        start, _size, name = max(containing, key=lambda item: item[0])
        symbol_text = f"{name}+0x{address - start:x}"
    tool = first_tool(ARCH_TO_ADDR2LINE[arch])
    if tool is None:
        return symbol_text
    result = subprocess.run(
        [tool, "-f", "-C", "-e", str(elf), f"0x{address:x}"],
        text=True,
        capture_output=True,
    )
    lines = result.stdout.strip().splitlines()
    location = lines[1] if len(lines) >= 2 else "??:0"
    return f"{symbol_text} at {location}"


@dataclass
class RemoteSample:
    stop_reason: str
    registers: list[dict[str, Any]]
    debug: dict[str, Any] | None
    build_id: str | None

    def signature(self) -> tuple[Any, ...]:
        cpu_signature = tuple(
            (item["thread"], item.get("pc"), item.get("sp")) for item in self.registers
        )
        if self.debug is None or "cpus" not in self.debug:
            return cpu_signature
        event_sequences = {
            item["cpu"]: item["next_sequence"]
            for item in self.debug.get("event_meta", [])
        }
        progress = tuple(
            (
                cpu["current_task"],
                cpu["timer_ticks"],
                cpu["context_switches"],
                cpu["syscalls"],
                cpu["traps"],
                cpu["ipi_sent"],
                cpu["ipi_received"],
                cpu["last_syscall_nr"],
                cpu["last_syscall_pc"],
                cpu["last_trap_pc"],
                event_sequences.get(cpu["cpu"], 0),
                tuple(cpu["runnable"]),
                cpu["waiting_lock"]["kind"],
                cpu["waiting_lock"]["object"],
            )
            for cpu in self.debug["cpus"]
            if cpu["online"]
        )
        return cpu_signature, progress


_REGISTER_DESCRIPTION_CACHE: dict[str, list[Any]] = {}


def remote_read_bytes(remote: GdbRemote, address: int, size: int) -> bytes | None:
    chunks = []
    offset = 0
    while offset < size:
        # QEMU advertises PacketSize=0x1000; memory replies are hex encoded, so
        # keep the binary request below roughly half that size plus framing.
        chunk_size = min(1900, size - offset)
        chunk = read_memory(remote, address + offset, chunk_size)
        if chunk is None or len(chunk) != chunk_size:
            return None
        chunks.append(chunk)
        offset += chunk_size
    return b"".join(chunks)


def collect_remote_sample(
    arch: str,
    elf: Path,
    host: str,
    port: int,
    timeout: float,
    *,
    leave_stopped: bool = False,
    full_events: bool = False,
) -> RemoteSample:
    remote: GdbRemote | None = None
    detached = False
    try:
        remote = GdbRemote(host, port, timeout)
        stop = remote.command("?")
        description = _REGISTER_DESCRIPTION_CACHE.get(arch)
        if description is None:
            description = read_register_description(remote)
            _REGISTER_DESCRIPTION_CACHE[arch] = description
        pc_reg = find_register(description, "pc", "era")
        sp_reg = find_register(description, "sp", "x2", "r3")
        ra_reg = find_register(description, "ra", "x1", "r1")
        fp_reg = find_register(description, "fp", "s0", "x8", "r22")
        if pc_reg is None:
            raise RemoteError("target description has no PC register")
        register_rows = []
        for cpu_index, thread in enumerate(read_threads(remote)):
            if remote.command(f"Hg{thread}") != "OK":
                raise RemoteError(f"cannot select QEMU thread {thread}")
            register_rows.append(
                {
                    "cpu": cpu_index,
                    "thread": thread,
                    "pc": read_register(remote, pc_reg),
                    "sp": read_register(remote, sp_reg) if sp_reg else None,
                    "ra": read_register(remote, ra_reg) if ra_reg else None,
                    "fp": read_register(remote, fp_reg) if fp_reg else None,
                }
            )

        debug_snapshot = None
        remote_build_id = None
        try:
            physical = remote.command("Qqemu.PhyMemMode:1")
            if physical != "OK":
                raise DebugToolError(f"QEMU does not support physical memory mode: {physical!r}")
            state_address, _ = nm_symbol(elf, arch, "WATEROS_DEBUG_STATE")
            header = remote_read_bytes(remote, state_address, HEADER_SIZE)
            if header is None:
                raise DebugAbiError("cannot read debug header")
            layout = parse_layout(header)
            expected_arch = 1 if arch == "rv" else 2
            if layout.arch != expected_arch:
                raise DebugToolError(
                    f"debug ABI architecture mismatch: requested={arch} header={layout.arch}"
                )
            # Watch mode only needs the compact CPU double-buffer area. Reading all
            # 32×256 event records through the stop-and-wait remote protocol would
            # pause the guest for tens of seconds on every sample.
            raw = bytearray(layout.total_size)
            raw[:HEADER_SIZE] = header
            # QEMU exposes one Remote thread per configured vCPU. CPU IDs are
            # contiguous in both supported machines, so sampling does not need to
            # fetch unused capacity slots (currently 32) for an `-smp 1/2/4/8`
            # guest. The decoder still sees a complete zero-filled ABI image.
            observed_cpus = min(layout.max_cpus, max(1, len(register_rows)))
            cpu_bytes = observed_cpus * layout.cpu_slots_size
            cpu_raw = remote_read_bytes(remote, state_address + HEADER_SIZE, cpu_bytes)
            if cpu_raw is None:
                raise DebugAbiError("cannot read CPU debug state")
            raw[HEADER_SIZE : HEADER_SIZE + cpu_bytes] = cpu_raw
            events_base = HEADER_SIZE + layout.max_cpus * layout.cpu_slots_size
            # 轻量采样也读取每 CPU 的 16-byte 环头，使 event sequence 能参与
            # 进展判定，而不必为每次 watch 传输完整的 256 项事件环。
            for cpu in range(observed_cpus):
                offset = events_base + cpu * layout.cpu_events_size
                event_header = remote_read_bytes(remote, state_address + offset, 16)
                if event_header is None:
                    raise DebugAbiError(f"cannot read debug event header for CPU {cpu}")
                raw[offset : offset + 16] = event_header
            debug_snapshot = decode_state(raw, event_limit=0)
            debug_snapshot["observed_vcpus"] = observed_cpus
            if full_events:
                for cpu_state in debug_snapshot["cpus"]:
                    if not cpu_state["online"]:
                        continue
                    cpu = cpu_state["cpu"]
                    offset = events_base + cpu * layout.cpu_events_size
                    event_raw = remote_read_bytes(
                        remote, state_address + offset, layout.cpu_events_size
                    )
                    if event_raw is None:
                        raise DebugAbiError(f"cannot read debug events for CPU {cpu}")
                    raw[offset : offset + layout.cpu_events_size] = event_raw
                debug_snapshot = decode_state(raw, event_limit=layout.event_capacity)
                debug_snapshot["observed_vcpus"] = observed_cpus
            build_address, _ = nm_symbol(elf, arch, "WATEROS_DEBUG_BUILD_ID")
            build_raw = remote_read_bytes(remote, build_address, layout.build_id_size)
            if build_raw:
                remote_build_id = build_raw.split(b"\0", 1)[0].decode("ascii", "replace")
            expected = local_build_id(elf, arch)
            verify_build_id(expected, remote_build_id)
            verify_build_id(remote_build_id, layout.build_id)
            debug_snapshot["build_id"] = remote_build_id
        except DebugAbiError as exc:
            raise DebugToolError(
                f"cannot decode WATEROS_DEBUG_STATE from the running guest: {exc}"
            ) from exc
        finally:
            try:
                remote.command("Qqemu.PhyMemMode:0")
            except Exception:
                pass

        if not leave_stopped:
            response = remote.command("D")
            detached = response == "OK"
        return RemoteSample(stop, register_rows, debug_snapshot, remote_build_id)
    finally:
        if remote is not None:
            if not leave_stopped and not detached:
                try:
                    remote.command("D")
                except Exception:
                    pass
            remote.close()


def gdb_command() -> str:
    tool = shutil.which("gdb-multiarch")
    if tool is None:
        raise DebugToolError(
            "gdb-multiarch is required; install with: sudo apt install gdb-multiarch"
        )
    return tool


def run_full_gdb(
    elf: Path, host: str, port: int, output: Path, *, leave_stopped: bool
) -> None:
    final_command = "disconnect" if leave_stopped else "detach"
    commands = [
        "set pagination off",
        "set confirm off",
        "set print pretty on",
        "set print frame-arguments all",
        f"file {elf.resolve()}",
        f"target remote {host}:{port}",
        f"source {GDB_EXTENSION.resolve()}",
        "wos-snapshot 128",
        "info threads",
        "thread apply all info all-registers",
        "thread apply all bt full",
        "thread apply all x/12i $pc-16",
        "thread apply all x/32gx $sp",
        final_command,
    ]
    argv = [gdb_command(), "--batch", "--nx"]
    for command in commands:
        argv.extend(["-ex", command])
    result = subprocess.run(argv, cwd=OS_ROOT, text=True, capture_output=True)
    output.write_text(result.stdout + "\n--- GDB STDERR ---\n" + result.stderr)
    if result.returncode != 0:
        raise DebugToolError(f"GDB snapshot failed; inspect {output}")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def report_directory(arch: str, build_id: str | None) -> Path:
    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    safe_build = (build_id or "unknown").replace("/", "_")
    path = OS_ROOT / "debug-reports" / f"{timestamp}-{arch}-{safe_build}"
    path.mkdir(parents=True, exist_ok=False)
    return path


def write_report(
    arch: str,
    elf: Path,
    sample: RemoteSample,
    reason: str,
    host: str,
    port: int,
    *,
    serial_log: Path | None = None,
    leave_stopped: bool = True,
) -> Path:
    report = report_directory(arch, sample.build_id)
    snapshot = {
        "stop_reason": sample.stop_reason,
        "registers": sample.registers,
        "debug": sample.debug,
    }
    (report / "snapshot.json").write_text(json.dumps(snapshot, indent=2))
    events = sample.debug.get("events", []) if sample.debug and "cpus" in sample.debug else []
    (report / "events.json").write_text(json.dumps(events, indent=2))
    metadata = {
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "arch": arch,
        "reason": reason,
        "host": host,
        "port": port,
        "elf": str(elf.resolve()),
        "elf_sha256": sha256(elf),
        "build_id": sample.build_id,
        "git_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=OS_ROOT, text=True
        ).strip(),
        "git_dirty": bool(
            subprocess.check_output(["git", "status", "--porcelain"], cwd=OS_ROOT)
        ),
        "gdb": subprocess.check_output([gdb_command(), "--version"], text=True)
        .splitlines()[0],
    }
    (report / "metadata.json").write_text(json.dumps(metadata, indent=2))
    register_note = (
        "RISC-V trap registers: sepc=return PC, scause=trap reason, "
        "stval=fault address, sstatus=privilege/interrupt state."
        if arch == "rv"
        else "LoongArch trap registers: era=return PC, estat=trap/interrupt status, "
        "badv=fault address, crmd/prmd=privilege/interrupt state."
    )
    summary_lines = [
        f"WaterOS hang diagnosis: {reason}",
        f"ELF: {elf}",
        register_note,
    ]
    if sample.debug and "cpus" in sample.debug:
        summary_lines.extend(["", render_cpus(sample.debug)])
        edges = lock_wait_edges(sample.debug)
        if edges:
            summary_lines.extend(["", "Lock wait edges:", json.dumps(edges, indent=2)])
    for row in sample.registers:
        location = (
            symbolize_address(str(elf.resolve()), arch, row["pc"])
            if row["pc"] is not None
            else "<unavailable>"
        )
        summary_lines.append(
            f"cpu-thread={row['thread']} pc={_hex(row['pc'])} sp={_hex(row['sp'])} "
            f"ra={_hex(row['ra'])} fp={_hex(row['fp'])} {location}"
        )
    (report / "summary.txt").write_text("\n".join(summary_lines) + "\n")
    (report / "reproduce.txt").write_text(
        f"./scripts/wateros_debug.py gdb --arch {arch} --elf {elf} "
        f"--host {host} --port {port}\n"
    )
    if serial_log and serial_log.exists():
        shutil.copyfile(serial_log, report / "serial.log")
        lines = serial_log.read_text(errors="replace").splitlines()[-300:]
        (report / "serial-tail.txt").write_text("\n".join(lines) + "\n")
    else:
        (report / "serial.log").write_text("")
        (report / "serial-tail.txt").write_text("")
    run_full_gdb(elf, host, port, report / "gdb.txt", leave_stopped=leave_stopped)
    return report


def _hex(value: int | None) -> str:
    return "-" if value is None else f"0x{value:016x}"


def is_quiescent(sample: RemoteSample) -> bool:
    if not sample.debug or "cpus" not in sample.debug:
        return False
    online = [cpu for cpu in sample.debug["cpus"] if cpu["online"]]
    return bool(online) and all(cpu["idle"] and not any(cpu["runnable"]) for cpu in online)


def stagnation_reasons(previous: RemoteSample, current: RemoteSample) -> set[str]:
    """Return per-CPU reasons that made no meaningful progress this interval.

    A global signature is insufficient on SMP: seven healthy/idle CPUs may keep
    ticking while one CPU is deadlocked.  Conversely, a CPU doing user computation
    is healthy when its PC or timer/trap progress changes.  Separate reason keys let
    ``watch`` require the *same* condition for the full confirmation window.
    """
    if (
        not previous.debug
        or "cpus" not in previous.debug
        or not current.debug
        or "cpus" not in current.debug
    ):
        return {"remote:fixed"} if previous.signature() == current.signature() else set()

    old_cpus = {cpu["cpu"]: cpu for cpu in previous.debug["cpus"]}
    old_registers = {
        row.get("cpu", index): row for index, row in enumerate(previous.registers)
    }
    new_registers = {
        row.get("cpu", index): row for index, row in enumerate(current.registers)
    }
    old_event_meta = {
        item["cpu"]: item for item in previous.debug.get("event_meta", [])
    }
    new_event_meta = {
        item["cpu"]: item for item in current.debug.get("event_meta", [])
    }
    reasons: set[str] = set()
    for cpu in current.debug["cpus"]:
        cpu_id = cpu["cpu"]
        old = old_cpus.get(cpu_id)
        if old is None or not cpu["online"]:
            continue
        waiting = cpu["waiting_lock"]
        relevant = (
            not cpu["idle"]
            or any(cpu["runnable"])
            or cpu["need_resched"]
            or bool(waiting["kind"])
        )
        if not relevant:
            continue
        prefix = f"cpu{cpu_id}"
        if cpu["timer_ticks"] == old["timer_ticks"]:
            reasons.add(f"{prefix}:timer")
        if waiting["kind"] and waiting == old["waiting_lock"]:
            reasons.add(f"{prefix}:lock")
        if (
            any(cpu["runnable"])
            and cpu["need_resched"]
            and cpu["current_task"] == old["current_task"]
            and cpu["context_switches"] == old["context_switches"]
        ):
            reasons.add(f"{prefix}:scheduler")

        old_reg = old_registers.get(cpu_id, {})
        new_reg = new_registers.get(cpu_id, {})
        old_seq = old_event_meta.get(cpu_id, {}).get("next_sequence")
        new_seq = new_event_meta.get(cpu_id, {}).get("next_sequence")
        semantic_fields = (
            "timer_ticks",
            "context_switches",
            "syscalls",
            "traps",
            "ipi_sent",
            "ipi_received",
        )
        if (
            old_reg.get("pc") == new_reg.get("pc")
            and old_reg.get("sp") == new_reg.get("sp")
            and all(cpu[field] == old[field] for field in semantic_fields)
            and old_seq == new_seq
        ):
            reasons.add(f"{prefix}:fixed")
    return reasons


def classify_stall(sample: RemoteSample, baseline: RemoteSample | None = None) -> str:
    if sample.debug and "cpus" in sample.debug:
        edges = lock_wait_edges(sample.debug)
        edge_map = {edge["waiter_cpu"]: edge["owner_cpu"] for edge in edges}
        for start in edge_map:
            seen = set()
            cpu = start
            while cpu is not None and cpu in edge_map:
                if cpu in seen:
                    return "lock-deadlock"
                seen.add(cpu)
                cpu = edge_map[cpu]
        if any(cpu["waiting_lock"]["kind"] for cpu in sample.debug["cpus"]):
            return "lock-wait-stall"
        fault_modes = {
            cpu["last_schedule_reason"] & 0xFFFF
            for cpu in sample.debug["cpus"]
            if cpu["last_schedule_reason"] & 0xFFFF0000 == DEBUG_FAULT_REASON_BASE
        }
        if 1 in fault_modes:
            return "fixed-pc-loop"
        if 2 in fault_modes:
            return "lock-deadlock"
        if 3 in fault_modes:
            return "interrupt-or-timer-stall"
        if 4 in fault_modes:
            return "scheduler-starvation"
        if any(any(cpu["runnable"]) and cpu["need_resched"] for cpu in sample.debug["cpus"]):
            return "scheduler-starvation"
        if baseline and baseline.debug and "cpus" in baseline.debug:
            old_by_cpu = {cpu["cpu"]: cpu for cpu in baseline.debug["cpus"]}
            if any(
                cpu["online"]
                and cpu["timer_ticks"] == old_by_cpu[cpu["cpu"]]["timer_ticks"]
                for cpu in sample.debug["cpus"]
            ):
                return "interrupt-or-timer-stall"
        recent_names = {event["name"] for event in sample.debug.get("events", [])[-16:]}
        if "tlb-shootdown" in recent_names:
            return "tlb-shootdown-wait"
        if "ipi-send" in recent_names and "ipi-receive" not in recent_names:
            return "ipi-delivery-wait"
    if baseline and [row.get("pc") for row in baseline.registers] == [
        row.get("pc") for row in sample.registers
    ]:
        return "fixed-pc-loop"
    if len({row.get("pc") for row in sample.registers}) <= 2:
        return "fixed-pc-loop"
    return "unknown-stall"


def watch(
    arch: str,
    elf: Path,
    host: str,
    port: int,
    interval: float,
    confirm: int,
    timeout: float,
    *,
    serial_log: Path | None = None,
    process: subprocess.Popen | None = None,
) -> int:
    previous_sample: RemoteSample | None = None
    reason_streaks: dict[str, int] = {}
    reason_baselines: dict[str, RemoteSample] = {}
    startup_deadline = time.monotonic() + 30
    while process is None or process.poll() is None:
        try:
            sample = collect_remote_sample(arch, elf, host, port, timeout)
        except (OSError, RemoteError) as exc:
            if time.monotonic() < startup_deadline:
                time.sleep(min(interval, 0.5))
                continue
            raise DebugToolError(f"cannot sample QEMU GDB stub: {exc}") from exc
        reasons = (
            stagnation_reasons(previous_sample, sample)
            if previous_sample is not None and not is_quiescent(sample)
            else set()
        )
        next_streaks: dict[str, int] = {}
        for reason_key in reasons:
            if reason_key in reason_streaks:
                next_streaks[reason_key] = reason_streaks[reason_key] + 1
            else:
                next_streaks[reason_key] = 1
                reason_baselines[reason_key] = previous_sample or sample
        reason_streaks = next_streaks
        for reason_key in list(reason_baselines):
            if reason_key not in reason_streaks:
                del reason_baselines[reason_key]
        previous_sample = sample
        leading_reason, stagnant = max(
            reason_streaks.items(), key=lambda item: item[1], default=("none", 0)
        )
        pcs = ", ".join(_hex(row["pc"]) for row in sample.registers)
        print(
            f"[wos-debug] stable={stagnant}/{confirm} reason={leading_reason} "
            f"pc=[{pcs}]",
            flush=True,
        )
        if stagnant >= confirm:
            baseline = reason_baselines.get(leading_reason)
            reason = classify_stall(sample, baseline)
            print(f"[wos-debug] confirmed {reason}; collecting full GDB report", flush=True)
            sample = collect_remote_sample(
                arch, elf, host, port, timeout, full_events=True
            )
            # The lightweight sample detached and resumed the guest. GDB performs the
            # authoritative all-stop capture and leaves it stopped afterwards.
            report = write_report(
                arch,
                elf,
                sample,
                reason,
                host,
                port,
                serial_log=serial_log,
                leave_stopped=True,
            )
            print(f"[wos-debug] report: {report}")
            print(
                f"[wos-debug] guest left stopped; continue with: "
                f"{gdb_command()} {elf} -ex 'target remote {host}:{port}'"
            )
            return 2
        time.sleep(interval)
    return process.returncode or 0


def doctor(arch: str | None, elf: Path | None) -> int:
    arches = [arch] if arch else ["rv", "la"]
    missing = []
    required = ["gdb-multiarch", "readelf", "python3"]
    required.extend(ARCH_TO_QEMU[item] for item in arches)
    for tool in required:
        path = shutil.which(tool)
        print(f"[{'OK' if path else 'MISSING'}] {tool}: {path or '-'}")
        if not path:
            missing.append(tool)
    for item in arches:
        tool = first_tool(ARCH_TO_BINUTILS[item])
        print(f"[{'OK' if tool else 'MISSING'}] {item} nm: {tool or '-'}")
        if not tool:
            missing.append(f"{item}-nm")
        addr2line = first_tool(ARCH_TO_ADDR2LINE[item])
        print(f"[{'OK' if addr2line else 'MISSING'}] {item} addr2line: {addr2line or '-'}")
        if not addr2line:
            missing.append(f"{item}-addr2line")
    if elf is not None:
        if not elf.is_file():
            print(f"[MISSING] ELF: {elf}")
            missing.append("elf")
        else:
            inferred = arch or ("la" if "la" in elf.name else "rv")
            try:
                build = local_build_id(elf, inferred)
                sections = subprocess.check_output(["readelf", "-S", str(elf)], text=True)
                has_debug = ".debug_info" in sections
                has_frame = ".debug_frame" in sections or ".eh_frame" in sections
                has_symbols = ".symtab" in sections
                has_frame_pointers = has_forced_frame_pointers(elf, inferred)
                missing_cfi = missing_cfi_symbols(elf, inferred) if has_frame else []
                print(f"[OK] build ID: {build}")
                print(f"[{'OK' if has_debug else 'MISSING'}] DWARF .debug_info")
                print(f"[{'OK' if has_frame else 'MISSING'}] frame information")
                print(f"[{'OK' if has_symbols else 'MISSING'}] ELF symbol table")
                print(
                    f"[{'OK' if not missing_cfi else 'MISSING'}] trap/switch/task CFI"
                    + (f": {', '.join(missing_cfi)}" if missing_cfi else "")
                )
                print(
                    f"[{'OK' if has_frame_pointers else 'MISSING'}] forced frame pointers"
                )
                if (
                    not has_debug
                    or not has_frame
                    or not has_symbols
                    or bool(missing_cfi)
                    or not has_frame_pointers
                ):
                    missing.append("elf-debug-info")
            except DebugToolError as exc:
                print(f"[INVALID] ELF: {exc}")
                missing.append("elf-debug-abi")
    if missing:
        print(
            "\nInstall host tools on Ubuntu with:\n"
            "  sudo apt install gdb-multiarch binutils-riscv64-unknown-elf "
            "binutils-loongarch64-linux-gnu qemu-system-misc"
        )
        return 2
    print("[wos-debug] doctor passed")
    return 0


def tee_output(process: subprocess.Popen, destination: Path) -> None:
    assert process.stdout is not None
    with destination.open("wb") as stream:
        while True:
            chunk = process.stdout.readline()
            if not chunk:
                break
            stream.write(chunk)
            stream.flush()
            sys.stdout.buffer.write(chunk)
            sys.stdout.buffer.flush()


def command_run(args: argparse.Namespace) -> int:
    arch, make_target, elf_name = PROFILES[args.profile]
    if doctor(arch, None) != 0:
        return 2
    elf = OS_ROOT / elf_name
    run_dir = OS_ROOT / "debug-reports" / "active"
    run_dir.mkdir(parents=True, exist_ok=True)
    serial_log = run_dir / f"{args.profile}-{datetime.now().strftime('%Y%m%d-%H%M%S')}.log"
    environment = os.environ.copy()
    environment.update(
        {
            "WOS_SMP": str(args.smp),
            "WOS_QEMU_GDB": "1",
            "WOS_QEMU_GDB_WAIT": "0",
            "WOS_QEMU_GDB_PORT": str(args.port),
            "WOS_QEMU_SNAPSHOT": "0" if args.write_disk else "1",
        }
    )
    previous_mtime = elf.stat().st_mtime_ns if elf.exists() else None
    process = subprocess.Popen(
        [
            "make",
            make_target,
            "GDB_WAIT=0",
            f"GDB_PORT={args.port}",
            f"GDB_FAULTS={1 if args.faults else 0}",
        ],
        cwd=OS_ROOT,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    output_thread = threading.Thread(target=tee_output, args=(process, serial_log), daemon=True)
    output_thread.start()
    # Build happens in the child; wait for the exact GDB ELF before sampling.
    deadline = time.monotonic() + args.build_timeout
    while process.poll() is None and time.monotonic() < deadline:
        if elf.is_file() and (previous_mtime is None or elf.stat().st_mtime_ns != previous_mtime):
            break
        time.sleep(0.25)
    if (not elf.is_file() or
        (previous_mtime is not None and elf.stat().st_mtime_ns == previous_mtime)):
        process.terminate()
        raise DebugToolError(f"debug ELF was not produced: {elf}")
    return watch(
        arch,
        elf,
        args.host,
        args.port,
        args.interval,
        args.confirm,
        args.timeout,
        serial_log=serial_log,
        process=process,
    )


def add_connection_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--arch", choices=["rv", "la"], required=True)
    parser.add_argument("--elf", type=Path, required=True)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=1234)
    parser.add_argument("--timeout", type=float, default=5.0)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    doctor_parser = subparsers.add_parser("doctor", help="validate host tools and an optional ELF")
    doctor_parser.add_argument("--arch", choices=["rv", "la"])
    doctor_parser.add_argument("--elf", type=Path)

    run_parser = subparsers.add_parser("run", help="build, launch and watch a debug kernel")
    run_parser.add_argument("profile", choices=sorted(PROFILES))
    run_parser.add_argument("--smp", type=int, default=8, choices=range(1, 9))
    run_parser.add_argument("--host", default="127.0.0.1")
    run_parser.add_argument("--port", type=int, default=1234)
    run_parser.add_argument("--interval", type=float, default=1.0)
    run_parser.add_argument("--confirm", type=int, default=10)
    run_parser.add_argument("--timeout", type=float, default=5.0)
    run_parser.add_argument("--build-timeout", type=float, default=600.0)
    run_parser.add_argument("--write-disk", action="store_true")
    run_parser.add_argument(
        "--faults",
        action="store_true",
        help="include test-only deterministic fault injection hooks",
    )

    snapshot_parser = subparsers.add_parser("snapshot", help="collect one complete report")
    add_connection_arguments(snapshot_parser)
    snapshot_parser.add_argument("--leave-stopped", action="store_true")
    snapshot_parser.add_argument("--serial-log", type=Path)

    watch_parser = subparsers.add_parser("watch", help="detect a hang and collect a report")
    add_connection_arguments(watch_parser)
    watch_parser.add_argument("--interval", type=float, default=1.0)
    watch_parser.add_argument("--confirm", type=int, default=10)
    watch_parser.add_argument("--serial-log", type=Path)

    gdb_parser = subparsers.add_parser("gdb", help="open interactive GDB with WaterOS commands")
    add_connection_arguments(gdb_parser)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        if args.command == "doctor":
            return doctor(args.arch, args.elf)
        if args.command == "run":
            return command_run(args)
        if not args.elf.is_file():
            raise DebugToolError(f"ELF not found: {args.elf}")
        # snapshot/watch 最终都必须执行完整的 all-thread GDB 抓取；在改变 guest
        # 状态前先失败，避免缺少依赖时留下一个半成品报告目录。
        gdb_command()
        if args.command == "snapshot":
            sample = collect_remote_sample(
                args.arch,
                args.elf,
                args.host,
                args.port,
                args.timeout,
                full_events=True,
            )
            report = write_report(
                args.arch,
                args.elf,
                sample,
                "manual-snapshot",
                args.host,
                args.port,
                serial_log=args.serial_log,
                leave_stopped=args.leave_stopped,
            )
            print((report / "summary.txt").read_text(), end="")
            print(f"[wos-debug] report: {report}")
            return 0
        if args.command == "watch":
            return watch(
                args.arch,
                args.elf,
                args.host,
                args.port,
                args.interval,
                args.confirm,
                args.timeout,
                serial_log=args.serial_log,
            )
        if args.command == "gdb":
            os.execv(
                gdb_command(),
                [
                    gdb_command(),
                    "--nx",
                    str(args.elf.resolve()),
                    "-ex",
                    f"target remote {args.host}:{args.port}",
                    "-ex",
                    f"source {GDB_EXTENSION.resolve()}",
                ],
            )
    except (DebugToolError, DebugAbiError, RemoteError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
