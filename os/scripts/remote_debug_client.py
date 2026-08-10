#!/usr/bin/env python3
"""Scriptable client for the opt-in WaterOS development TCP monitor.

This is not an SSH client: the guest service has no authentication or encryption.
Use it only on a trusted bring-up network or through QEMU's loopback-only forwarding.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import socket
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

PROMPT = b"wos> "
MAX_RESPONSE = 64 * 1024
MAX_COMMAND_LEN = 128
BOARD_ID_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}\Z")


class MonitorProtocolError(RuntimeError):
    pass


@dataclass(frozen=True)
class CommandResult:
    command: str
    response: str


@dataclass(frozen=True)
class MmcEvidence:
    """Parsed index for one raw, read-only ``ls2k-mmc`` response."""

    fields: dict[str, str]
    gates: dict[str, str]
    controller: dict[str, int | str | None]


class MonitorClient:
    def __init__(self, connection: socket.socket, timeout: float = 5.0) -> None:
        self._connection = connection
        self._connection.settimeout(timeout)
        self._banner: str | None = None

    def close(self) -> None:
        self._connection.close()

    def _receive_until(self, marker: bytes) -> bytes:
        data = bytearray()
        while marker not in data:
            chunk = self._connection.recv(4096)
            if not chunk:
                raise MonitorProtocolError(
                    f"connection closed before marker {marker!r}; received {bytes(data)!r}"
                )
            data.extend(chunk)
            if len(data) > MAX_RESPONSE:
                raise MonitorProtocolError("monitor response exceeds 64 KiB limit")
        marker_offset = data.index(marker)
        trailing = data[marker_offset + len(marker) :]
        if trailing:
            raise MonitorProtocolError(f"unexpected bytes after prompt: {trailing!r}")
        return bytes(data[:marker_offset])

    def receive_banner(self) -> str:
        if self._banner is not None:
            return self._banner
        banner = self._receive_until(PROMPT)
        if not banner.startswith(b"WaterOS development monitor\r\n"):
            raise MonitorProtocolError(f"unexpected banner: {banner!r}")
        self._banner = banner.decode("utf-8", errors="strict")
        return self._banner

    def command(self, command: str) -> CommandResult:
        if "\n" in command or "\r" in command:
            raise ValueError("one monitor command must fit on one line")
        encoded = command.encode("utf-8")
        if len(encoded) > MAX_COMMAND_LEN:
            raise ValueError("monitor command exceeds 128-byte limit")
        self._connection.sendall(encoded + b"\n")
        response = self._receive_until(PROMPT)
        return CommandResult(command, response.decode("utf-8", errors="strict"))

    def quit(self) -> CommandResult:
        self._connection.sendall(b"quit\n")
        response = self._receive_until(b"bye\r\n") + b"bye\r\n"
        return CommandResult("quit", response.decode("utf-8", errors="strict"))


def connect_with_retry(host: str, port: int, timeout: float) -> MonitorClient:
    deadline = time.monotonic() + timeout
    last_error: BaseException | None = None
    while time.monotonic() < deadline:
        try:
            connection = socket.create_connection((host, port), timeout=min(1.0, timeout))
            client = MonitorClient(connection, timeout=min(2.0, timeout))
            try:
                client.receive_banner()
                return client
            except (OSError, MonitorProtocolError, UnicodeError):
                client.close()
                raise
        except (OSError, MonitorProtocolError, UnicodeError) as error:
            last_error = error
            time.sleep(0.1)
    raise TimeoutError(f"monitor {host}:{port} did not become ready: {last_error}")


def run_smoke(client: MonitorClient) -> list[CommandResult]:
    client.receive_banner()
    results = [
        client.command("ping"),
        client.command("status"),
        client.command("version"),
        client.command("devfs"),
        client.command("ls2k-mmc"),
        client.command("reboot"),
    ]
    if results[0].response != "pong\r\n":
        raise MonitorProtocolError(f"ping failed: {results[0].response!r}")
    required_status = ("tick=", "online_cpus=", "heap_used=", "heap_free=", "heap_capacity=")
    if not all(field in results[1].response for field in required_status):
        raise MonitorProtocolError(f"incomplete status: {results[1].response!r}")
    if not results[2].response.startswith("WaterOS "):
        raise MonitorProtocolError(f"version failed: {results[2].response!r}")
    if not results[3].response.startswith("devfs generation="):
        raise MonitorProtocolError(f"invalid devfs response: {results[3].response!r}")
    mmc_prefixes = (
        "ls2k-mmc ",
        "ERR ls2k-mmc ",
        "ERR unavailable: ls2k-mmc ",
        "ERR unsupported: ls2k-mmc ",
    )
    if not results[4].response.startswith(mmc_prefixes):
        raise MonitorProtocolError(f"invalid ls2k-mmc response: {results[4].response!r}")
    if results[5].response != "unknown command; type 'help'\r\n":
        raise MonitorProtocolError(f"unknown-command guard failed: {results[5].response!r}")
    results.append(client.quit())
    return results


def _parse_binary(name: str, value: str) -> int:
    if value not in ("0", "1"):
        raise MonitorProtocolError(f"ls2k-mmc {name} must be 0 or 1: {value!r}")
    return int(value)


def _parse_hex_or_na(name: str, value: str) -> int | None:
    if value == "na":
        return None
    if not value.startswith("0x"):
        raise MonitorProtocolError(f"ls2k-mmc {name} must be hexadecimal or na: {value!r}")
    try:
        return int(value, 16)
    except ValueError as error:
        raise MonitorProtocolError(f"ls2k-mmc {name} has invalid hexadecimal value") from error


def _valid_board_id(board_id: str) -> bool:
    return BOARD_ID_PATTERN.fullmatch(board_id) is not None


def parse_mmc_evidence(response: str) -> MmcEvidence:
    """Validate and index a successful controller-evidence response.

    The original response remains the authoritative evidence. Unknown fields are
    retained in ``fields`` so a newer monitor can extend the line without making
    an older capture client discard evidence.
    """
    if not response.endswith("\r\n") or "\r\n" in response[:-2]:
        raise MonitorProtocolError("ls2k-mmc evidence must be exactly one CRLF-terminated line")
    tokens = response[:-2].split(" ")
    if not tokens or tokens[0] != "ls2k-mmc" or any(not token for token in tokens):
        raise MonitorProtocolError("invalid ls2k-mmc evidence prefix or spacing")
    fields: dict[str, str] = {}
    for token in tokens[1:]:
        if "=" not in token:
            raise MonitorProtocolError(f"invalid ls2k-mmc field: {token!r}")
        name, value = token.split("=", 1)
        if not name or not value or name in fields:
            raise MonitorProtocolError(f"invalid or duplicate ls2k-mmc field: {name!r}")
        fields[name] = value

    required = {
        "clock", "vmmc", "vqmmc", "pinctrl", "pinmux", "card", "gates",
        "proof", "can_activate", "blockers", "controller", "trace", "assessment",
    }
    missing = sorted(required - fields.keys())
    if missing:
        raise MonitorProtocolError(f"ls2k-mmc evidence missing fields: {','.join(missing)}")
    _parse_binary("proof", fields["proof"])
    _parse_binary("can_activate", fields["can_activate"])
    try:
        blockers = int(fields["blockers"], 10)
    except ValueError as error:
        raise MonitorProtocolError("ls2k-mmc blockers must be decimal") from error
    if blockers < 0:
        raise MonitorProtocolError("ls2k-mmc blockers must be non-negative")

    gates: dict[str, str] = {}
    for item in fields["gates"].split(","):
        if ":" not in item:
            raise MonitorProtocolError(f"invalid ls2k-mmc gate: {item!r}")
        name, value = item.split(":", 1)
        if not name or not value or name in gates:
            raise MonitorProtocolError(f"invalid or duplicate ls2k-mmc gate: {name!r}")
        gates[name] = value
    required_gates = {"clock", "vmmc", "vqmmc", "pinctrl", "card", "irq"}
    if required_gates - gates.keys():
        raise MonitorProtocolError("ls2k-mmc evidence has incomplete prerequisite gates")
    gate_states = {
        "satisfied", "observed-only", "unverified-hardware", "blocked", "missing",
        "unsupported", "error",
    }
    invalid_gates = sorted(name for name, value in gates.items() if value not in gate_states)
    if invalid_gates:
        raise MonitorProtocolError(
            f"ls2k-mmc evidence has invalid gate states: {','.join(invalid_gates)}"
        )

    controller_state = fields["controller"]
    if controller_state != "ok" and not controller_state.startswith("error:"):
        raise MonitorProtocolError(f"invalid ls2k-mmc controller state: {controller_state!r}")
    register_names = ("carg", "cctl", "csts", "dsts", "int")
    controller: dict[str, int | str | None] = {"state": controller_state}
    for name in register_names:
        if name not in fields:
            raise MonitorProtocolError(f"ls2k-mmc controller evidence missing {name}")
        controller[name] = _parse_hex_or_na(name, fields[name])
    if controller_state == "ok":
        if any(controller[name] is None for name in register_names):
            raise MonitorProtocolError("successful ls2k-mmc controller evidence contains na")
        for name in ("idle", "clean"):
            if name not in fields:
                raise MonitorProtocolError(f"ls2k-mmc controller evidence missing {name}")
            controller[name] = _parse_binary(name, fields[name])
        for name in ("int_known", "int_unknown"):
            if name not in fields:
                raise MonitorProtocolError(f"ls2k-mmc controller evidence missing {name}")
            controller[name] = _parse_hex_or_na(name, fields[name])
    if fields["trace"] != "none" or fields["assessment"] != "unavailable":
        raise MonitorProtocolError(
            "capture client does not yet support command-bearing ls2k-mmc evidence"
        )
    return MmcEvidence(fields=fields, gates=gates, controller=controller)


def write_mmc_evidence(path: Path, board_id: str, response: str,
                       *, captured_at: datetime | None = None) -> None:
    """Create, without overwriting, a compact host-side evidence record."""
    if not _valid_board_id(board_id):
        raise ValueError("board_id must use 1..128 ASCII letters, digits, dot, underscore, or dash")
    evidence = parse_mmc_evidence(response)
    timestamp = captured_at or datetime.now(timezone.utc)
    if timestamp.tzinfo is None:
        raise ValueError("captured_at must be timezone-aware")
    record: dict[str, Any] = {
        "schema": "wateros-ls2k-mmc-evidence-v1",
        "board_id": board_id,
        "captured_at": timestamp.astimezone(timezone.utc).isoformat().replace("+00:00", "Z"),
        "command": "ls2k-mmc",
        "response": response,
        "response_sha256": hashlib.sha256(response.encode("utf-8")).hexdigest(),
        "parsed": {
            "fields": evidence.fields,
            "gates": evidence.gates,
            "controller": evidence.controller,
        },
        "hardware_validation": "unverified-observation",
    }
    with path.open("x", encoding="utf-8", newline="\n") as output:
        json.dump(record, output, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
        output.write("\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=22323)
    parser.add_argument("--connect-timeout", type=float, default=30.0)
    parser.add_argument("--mmc-evidence", type=Path,
                        help="create a compact JSON record from the ls2k-mmc response")
    parser.add_argument("--board-id",
                        help="physical board identifier recorded with --mmc-evidence")
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("--port must be in 1..65535")
    if args.connect_timeout <= 0:
        parser.error("--connect-timeout must be positive")
    if (args.mmc_evidence is None) != (args.board_id is None):
        parser.error("--mmc-evidence and --board-id must be used together")
    if args.board_id is not None and not _valid_board_id(args.board_id):
        parser.error("--board-id must use 1..128 ASCII letters, digits, dot, underscore, or dash")

    client = connect_with_retry(args.host, args.port, args.connect_timeout)
    try:
        results = run_smoke(client)
    finally:
        client.close()
    if args.mmc_evidence is not None:
        mmc_result = next(result for result in results if result.command == "ls2k-mmc")
        write_mmc_evidence(args.mmc_evidence, args.board_id, mmc_result.response)
    for result in results:
        print(f"[{result.command}] {result.response.rstrip()}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, TimeoutError, MonitorProtocolError, UnicodeError) as error:
        print(f"remote-debug smoke failed: {error}", file=sys.stderr)
        raise SystemExit(1)
