#!/usr/bin/env python3
"""Stop a QEMU guest through its GDB stub and snapshot every CPU.

This is intentionally a small GDB Remote client instead of a debugger
replacement.  It is useful on hosts whose GDB/LLDB does not understand the
guest architecture (notably Apple LLDB with LoongArch).
"""
from __future__ import annotations

import argparse
import re
import socket
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path

_DEBUG_DIR = Path(__file__).resolve().parent / "debug"
if str(_DEBUG_DIR) not in sys.path:
    sys.path.insert(0, str(_DEBUG_DIR))

from symbol_index import SymbolIndex  # noqa: E402


@dataclass(frozen=True)
class Register:
    name: str
    number: int
    bitsize: int


class RemoteError(RuntimeError):
    pass


class GdbRemote:
    def __init__(self, host: str, port: int, timeout: float) -> None:
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.sock.settimeout(timeout)
        self._buffer = bytearray()

    def close(self) -> None:
        self.sock.close()

    @staticmethod
    def _frame(payload: str) -> bytes:
        data = payload.encode("ascii")
        checksum = sum(data) & 0xFF
        return b"$" + data + f"#{checksum:02x}".encode("ascii")

    def command(self, payload: str) -> str:
        self.sock.sendall(self._frame(payload))
        return self._read_packet()

    def _read_byte(self) -> int:
        if not self._buffer:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise RemoteError("QEMU closed the GDB connection")
            self._buffer.extend(chunk)
        value = self._buffer[0]
        del self._buffer[0]
        return value

    def _read_packet(self) -> str:
        while self._read_byte() != ord("$"):
            pass
        payload = bytearray()
        while True:
            byte = self._read_byte()
            if byte == ord("#"):
                break
            payload.append(byte)
        expected = bytes((self._read_byte(), self._read_byte()))
        actual = f"{sum(payload) & 0xFF:02x}".encode("ascii")
        if expected.lower() != actual:
            self.sock.sendall(b"-")
            raise RemoteError(
                f"bad GDB packet checksum: expected {expected!r}, calculated {actual!r}"
            )
        self.sock.sendall(b"+")
        return payload.decode("ascii")

    def read_feature(self, annex: str) -> str:
        offset = 0
        chunks: list[str] = []
        while True:
            response = self.command(
                f"qXfer:features:read:{annex}:{offset:x},1000"
            )
            if not response or response[0] not in ("m", "l"):
                raise RemoteError(
                    f"cannot read target feature {annex!r}: {response!r}"
                )
            chunks.append(response[1:])
            offset += len(response[1:].encode("ascii"))
            if response[0] == "l":
                return "".join(chunks)


def _local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def read_register_description(remote: GdbRemote) -> list[Register]:
    pending = ["target.xml"]
    seen: set[str] = set()
    registers: list[Register] = []
    next_number = 0
    while pending:
        annex = pending.pop(0)
        if annex in seen:
            continue
        seen.add(annex)
        xml = remote.read_feature(annex)
        # QEMU's generated target.xml uses xi:include without declaring the
        # xi namespace.  GDB accepts it; ElementTree correctly rejects it.
        xml = re.sub(r"(<\/?)xi:", r"\1", xml)
        root = ET.fromstring(xml)
        for elem in root.iter():
            if _local_name(elem.tag) == "include":
                href = elem.attrib.get("href")
                if href:
                    pending.append(href)
            elif _local_name(elem.tag) == "reg":
                number = int(elem.attrib.get("regnum", next_number))
                bitsize = int(elem.attrib["bitsize"])
                registers.append(Register(elem.attrib["name"], number, bitsize))
                next_number = number + 1
    return registers


def read_threads(remote: GdbRemote) -> list[str]:
    response = remote.command("qfThreadInfo")
    threads: list[str] = []
    while response.startswith("m"):
        threads.extend(t for t in response[1:].split(",") if t)
        response = remote.command("qsThreadInfo")
    if response != "l":
        raise RemoteError(f"cannot enumerate QEMU CPUs: {response!r}")
    return threads


def read_register(remote: GdbRemote, register: Register) -> int | None:
    response = remote.command(f"p{register.number:x}")
    if response.startswith("E") or response.startswith("x"):
        return None
    try:
        raw = bytes.fromhex(response)
    except ValueError:
        return None
    return int.from_bytes(raw, "little")


def read_memory(remote: GdbRemote, address: int, size: int) -> bytes | None:
    response = remote.command(f"m{address:x},{size:x}")
    if response.startswith("E"):
        return None
    try:
        return bytes.fromhex(response)
    except ValueError:
        return None


def find_register(
    registers: list[Register], *names: str
) -> Register | None:
    by_name = {register.name.lower(): register for register in registers}
    for name in names:
        if name.lower() in by_name:
            return by_name[name.lower()]
    return None


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Snapshot all QEMU vCPUs through a GDB Remote port"
    )
    parser.add_argument("--arch", choices=["rv", "la"], required=True)
    parser.add_argument("--elf", type=Path, required=True)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=1234)
    parser.add_argument("--timeout", type=float, default=5.0)
    parser.add_argument(
        "--stack-words",
        type=int,
        default=64,
        help="Scan this many words above SP for kernel text addresses (default: 64)",
    )
    parser.add_argument(
        "--leave-stopped",
        action="store_true",
        help="Do not detach/resume QEMU after collecting the snapshot",
    )
    args = parser.parse_args()

    if not args.elf.is_file():
        print(f"error: ELF not found: {args.elf}", file=sys.stderr)
        return 2

    remote: GdbRemote | None = None
    detached = False
    try:
        remote = GdbRemote(args.host, args.port, args.timeout)
        # QEMU enters the all-stop state when a debugger connects.  Querying
        # the stop reason is more portable than sending a raw Ctrl-C before
        # the first framed packet (some QEMU targets ignore that first byte).
        stop = remote.command("?")
        print(f"[gdb-snapshot] guest stopped: {stop}")

        registers = read_register_description(remote)
        if args.arch == "la":
            pc_reg = find_register(registers, "pc")
            ra_reg = find_register(registers, "r1", "ra")
            sp_reg = find_register(registers, "r3", "sp")
            fp_reg = find_register(registers, "r22", "fp")
            badv_reg = find_register(registers, "badv")
            diagnostic_regs = [
                register
                for name in ("era", "estat", "prmd", "crmd", "ecfg", "eentry")
                if (register := find_register(registers, name)) is not None
            ]
        else:
            pc_reg = find_register(registers, "pc")
            ra_reg = find_register(registers, "ra", "x1")
            sp_reg = find_register(registers, "sp", "x2")
            fp_reg = find_register(registers, "fp", "s0", "x8")
            badv_reg = find_register(registers, "badv", "stval")
            diagnostic_regs = []
        if pc_reg is None:
            names = ", ".join(register.name for register in registers)
            raise RemoteError(f"target XML has no PC register (registers: {names})")

        index = SymbolIndex(args.elf.resolve(), args.arch)
        threads = read_threads(remote)
        print(f"[gdb-snapshot] {len(threads)} CPU thread(s)")
        for cpu_index, thread_id in enumerate(threads):
            selected = remote.command(f"Hg{thread_id}")
            if selected != "OK":
                raise RemoteError(
                    f"cannot select CPU thread {thread_id}: {selected!r}"
                )
            pc = read_register(remote, pc_reg)
            ra = read_register(remote, ra_reg) if ra_reg else None
            sp = read_register(remote, sp_reg) if sp_reg else None
            fp = read_register(remote, fp_reg) if fp_reg else None
            badv = read_register(remote, badv_reg) if badv_reg else None
            if pc is None:
                print(f"cpu={cpu_index} thread={thread_id} pc=<unavailable>")
                continue
            pc_lookup = index.lookup_fast(pc)
            location = pc_lookup.format_short()
            ra_text = f"0x{ra:016x}" if ra is not None else "<unavailable>"
            sp_text = f"0x{sp:016x}" if sp is not None else "<unavailable>"
            fp_text = f"0x{fp:016x}" if fp is not None else "<unavailable>"
            badv_text = (
                f" badv=0x{badv:016x}" if badv is not None else ""
            )
            diagnostic_text = "".join(
                f" {register.name}=0x{value:016x}"
                for register in diagnostic_regs
                if (value := read_register(remote, register)) is not None
            )
            print(
                f"cpu={cpu_index} thread={thread_id} "
                f"pc=0x{pc:016x} ra={ra_text} sp={sp_text} fp={fp_text}"
                f"{badv_text}{diagnostic_text}"
            )
            print(f"  {location}")
            if (
                sp is not None
                and pc_lookup.symbol is not None
                and "fatal_kernel_trap" in pc_lookup.symbol.name
            ):
                fatal_frame = read_memory(remote, sp + 16, 24)
                if fatal_frame is not None and len(fatal_frame) == 24:
                    raw_cause = int.from_bytes(fatal_frame[0:8], "little")
                    trapped_pc = int.from_bytes(fatal_frame[8:16], "little")
                    fault_addr = int.from_bytes(fatal_frame[16:24], "little")
                    trapped_location = index.lookup_fast(trapped_pc).format_short()
                    print(
                        f"  fatal: raw_cause=0x{raw_cause:x} "
                        f"trapped_pc=0x{trapped_pc:016x} "
                        f"fault_addr=0x{fault_addr:016x}"
                    )
                    print(f"  trapped-at: {trapped_location}")
            if sp is not None and args.stack_words > 0:
                stack = read_memory(remote, sp, args.stack_words * 8)
                if stack is not None:
                    for offset in range(0, len(stack) - 7, 8):
                        value = int.from_bytes(stack[offset : offset + 8], "little")
                        lookup = index.lookup_fast(value)
                        if (
                            lookup.symbol is not None
                            and lookup.symbol.kind in "Tt"
                        ):
                            print(
                                f"  stack+0x{offset:03x}: "
                                f"{lookup.format_short()}"
                            )

        if not args.leave_stopped:
            response = remote.command("D")
            detached = response == "OK"
            if not detached:
                raise RemoteError(f"QEMU refused detach/resume: {response!r}")
            print("[gdb-snapshot] detached; guest resumed")
        else:
            print("[gdb-snapshot] guest remains stopped")
        return 0
    except (OSError, ET.ParseError, RemoteError, RuntimeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    finally:
        if remote is not None:
            if not args.leave_stopped and not detached:
                try:
                    remote.command("D")
                except (OSError, RemoteError):
                    pass
            remote.close()


if __name__ == "__main__":
    raise SystemExit(main())
