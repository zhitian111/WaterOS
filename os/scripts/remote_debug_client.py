#!/usr/bin/env python3
"""Scriptable client for the opt-in WaterOS development TCP monitor.

This is not an SSH client: the guest service has no authentication or encryption.
Use it only on a trusted bring-up network or through QEMU's loopback-only forwarding.
"""

from __future__ import annotations

import argparse
import socket
import sys
import time
from dataclasses import dataclass

PROMPT = b"wos> "
MAX_RESPONSE = 64 * 1024


class MonitorProtocolError(RuntimeError):
    pass


@dataclass(frozen=True)
class CommandResult:
    command: str
    response: str


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
        self._connection.sendall(command.encode("utf-8") + b"\n")
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
        client.command("ls2k-mmc"),
    ]
    if results[0].response != "pong\r\n":
        raise MonitorProtocolError(f"ping failed: {results[0].response!r}")
    required_status = ("tick=", "online_cpus=", "heap_used=", "heap_free=", "heap_capacity=")
    if not all(field in results[1].response for field in required_status):
        raise MonitorProtocolError(f"incomplete status: {results[1].response!r}")
    if not results[2].response.startswith("WaterOS "):
        raise MonitorProtocolError(f"version failed: {results[2].response!r}")
    mmc_prefixes = (
        "ls2k-mmc ",
        "ERR ls2k-mmc ",
        "ERR unavailable: ls2k-mmc ",
        "ERR unsupported: ls2k-mmc ",
    )
    if not results[3].response.startswith(mmc_prefixes):
        raise MonitorProtocolError(f"invalid ls2k-mmc response: {results[3].response!r}")
    results.append(client.quit())
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=22323)
    parser.add_argument("--connect-timeout", type=float, default=30.0)
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("--port must be in 1..65535")
    if args.connect_timeout <= 0:
        parser.error("--connect-timeout must be positive")

    client = connect_with_retry(args.host, args.port, args.connect_timeout)
    try:
        results = run_smoke(client)
    finally:
        client.close()
    for result in results:
        print(f"[{result.command}] {result.response.rstrip()}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, TimeoutError, MonitorProtocolError, UnicodeError) as error:
        print(f"remote-debug smoke failed: {error}", file=sys.stderr)
        raise SystemExit(1)
