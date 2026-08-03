#!/usr/bin/env python3
"""Small GDB Remote protocol primitives used by ``wateros_debug.py``.

This module intentionally has no command-line interface.  Build/attach/watch/report
policy belongs to the unified ``scripts/wateros_debug.py`` entry point; keeping the
packet client here makes it independently unit-testable without becoming a second
debug tool with subtly different defaults.
"""
from __future__ import annotations

import re
import socket
import xml.etree.ElementTree as ET
from dataclasses import dataclass


@dataclass(frozen=True)
class Register:
    name: str
    number: int
    bitsize: int


class RemoteError(RuntimeError):
    pass


class GdbRemote:
    """Minimal acknowledged-mode client for QEMU's all-stop GDB stub."""

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
        # Ignore leading '+' acknowledgements and asynchronous noise until the
        # next framed reply begins.
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
            response = self.command(f"qXfer:features:read:{annex}:{offset:x},1000")
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
    """Read target.xml and every included register feature from QEMU."""
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
        # Some generated QEMU XML uses xi:include without declaring xi.
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
        threads.extend(thread for thread in response[1:].split(",") if thread)
        response = remote.command("qsThreadInfo")
    if response != "l":
        raise RemoteError(f"cannot enumerate QEMU CPUs: {response!r}")
    return threads


def read_register(remote: GdbRemote, register: Register | None) -> int | None:
    if register is None:
        return None
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


def find_register(registers: list[Register], *names: str) -> Register | None:
    by_name = {register.name.lower(): register for register in registers}
    return next(
        (by_name[name.lower()] for name in names if name.lower() in by_name),
        None,
    )


__all__ = [
    "GdbRemote",
    "Register",
    "RemoteError",
    "find_register",
    "read_memory",
    "read_register",
    "read_register_description",
    "read_threads",
]
