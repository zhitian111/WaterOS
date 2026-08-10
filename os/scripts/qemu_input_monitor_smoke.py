#!/usr/bin/env python3
"""Smoke-test QEMU's monitor-side input injection path.

This deliberately does not claim that WaterOS received a guest event.  It only
checks that QEMU accepts an HMP ``sendkey`` command, which is useful before a
physical board is available.
"""
from __future__ import annotations

import argparse
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path


def build_command(qemu: str, monitor: Path) -> list[str]:
    return [
        qemu,
        "-machine",
        "q35",
        "-display",
        "none",
        "-monitor",
        f"unix:{monitor},server=on,wait=off",
        "-S",
    ]


def response_is_success(response: bytes) -> bool:
    text = response.decode("utf-8", errors="replace").lower()
    return "error" not in text and "unknown command" not in text


def _read_prompt(conn: socket.socket, timeout: float) -> bytes:
    conn.settimeout(timeout)
    data = bytearray()
    while b"(qemu)" not in data:
        chunk = conn.recv(4096)
        if not chunk:
            break
        data.extend(chunk)
    return bytes(data)


def run(qemu: str, timeout: float = 3.0) -> tuple[bool, str]:
    with tempfile.TemporaryDirectory(prefix="wateros-qemu-monitor-") as directory:
        monitor = Path(directory) / "monitor.sock"
        process = subprocess.Popen(
            build_command(qemu, monitor),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        try:
            deadline = time.monotonic() + timeout
            while not monitor.exists() and time.monotonic() < deadline:
                if process.poll() is not None:
                    break
                time.sleep(0.02)
            if not monitor.exists():
                detail = process.stderr.read().decode(errors="replace")
                return False, f"monitor socket 未创建: {detail.strip()}"
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
                conn.connect(str(monitor))
                _read_prompt(conn, timeout)
                conn.sendall(b"sendkey a\n")
                response = _read_prompt(conn, timeout)
                if not response_is_success(response):
                    return False, response.decode(errors="replace")
                conn.sendall(b"info status\n")
                status = _read_prompt(conn, timeout)
                if not response_is_success(status):
                    return False, status.decode(errors="replace")
                conn.sendall(b"quit\n")
            process.wait(timeout=timeout)
            return True, "QEMU HMP send-key monitor smoke passed"
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    process.kill()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qemu", default="qemu-system-x86_64")
    args = parser.parse_args()
    qemu = shutil.which(args.qemu)
    if qemu is None:
        print(f"SKIP: 未找到 QEMU: {args.qemu}")
        return 77
    passed, message = run(qemu)
    print(message)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
