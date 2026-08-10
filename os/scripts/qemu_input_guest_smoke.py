#!/usr/bin/env python3
"""Verify QEMU virtio keyboard/tablet enumeration inside a WaterOS guest."""

from __future__ import annotations

import argparse
import os
import shutil
import json
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))

from qemu_run import build_qemu_launch

QEMU_BINARIES = {"rv": "qemu-system-riscv64", "la": "qemu-system-loongarch64"}
REQUIRED_MARKERS = (
    "registered virtio-input #0",
    "registered virtio-input #1",
    "input devices registered: count=2",
    "input=2",
    "[gui] input events received=",
)


def diagnose_inputs(arch: str, kernel: Path, sdcard: Path) -> list[str]:
    errors: list[str] = []
    if arch not in QEMU_BINARIES:
        errors.append(f"unsupported architecture: {arch}")
    if not kernel.is_file():
        errors.append(f"kernel not found: {kernel}")
    elif kernel.stat().st_size == 0:
        errors.append(f"kernel is empty: {kernel}")
    if not sdcard.is_file():
        errors.append(f"sdcard not found: {sdcard}")
    elif sdcard.stat().st_size == 0:
        errors.append(f"sdcard is empty: {sdcard}")
    return errors


def missing_markers(serial: str) -> list[str]:
    return [marker for marker in REQUIRED_MARKERS if marker not in serial]


def inject_qmp_events(socket_path: Path, timeout: float = 2.0) -> tuple[bool, str]:
    """Inject one key press/release and one tablet motion through QMP."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
                conn.settimeout(max(0.1, deadline - time.monotonic()))
                conn.connect(str(socket_path))
                greeting = conn.makefile("rb").readline()
                if b"QMP" not in greeting:
                    return False, f"invalid QMP greeting: {greeting!r}"
                def request(payload: dict) -> dict:
                    conn.sendall((json.dumps(payload) + "\n").encode())
                    line = conn.makefile("rb").readline()
                    return json.loads(line)
                reply = request({"execute": "qmp_capabilities"})
                if "error" in reply:
                    return False, f"qmp_capabilities failed: {reply['error']}"
                events = [
                    {"type": "key", "data": {"down": True, "key": {"type": "qcode", "data": "a"}}},
                    {"type": "key", "data": {"down": False, "key": {"type": "qcode", "data": "a"}}},
                    {"type": "abs", "data": {"axis": "x", "value": 100}},
                    {"type": "abs", "data": {"axis": "y", "value": 100}},
                ]
                reply = request({"execute": "input-send-event", "arguments": {"events": events}})
                if "error" in reply:
                    return False, f"input-send-event failed: {reply['error']}"
                return True, "QMP input events injected"
        except (FileNotFoundError, ConnectionRefusedError, OSError):
            time.sleep(0.02)
    return False, "QMP socket did not become ready"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=("rv", "la"), default="rv")
    parser.add_argument("--profile", choices=("pre", "final"), default="final")
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--sdcard", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=10.0)
    args = parser.parse_args()
    errors = diagnose_inputs(args.arch, args.kernel, args.sdcard)
    if errors:
        print("SKIP: QEMU guest 产物/参数尚未准备好:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 77
    qemu = shutil.which(QEMU_BINARIES[args.arch])
    if qemu is None:
        print(f"SKIP: 未找到 {QEMU_BINARIES[args.arch]}", file=sys.stderr)
        return 77
    if args.timeout <= 0:
        parser.error("--timeout must be positive")

    environment = dict(os.environ)
    environment.update({
        "WOS_KERNEL": str(args.kernel.resolve()),
        "WOS_SDCARD": str(args.sdcard.resolve()),
        "WOS_SMP": "1",
        "WOS_QEMU_SNAPSHOT": "1",
        "WOS_GRAPHICS": "1",
        "WOS_QEMU_DISPLAY": "none",
    })
    with tempfile.TemporaryDirectory(prefix="wateros-qemu-input-") as directory:
        qmp_socket = Path(directory) / "qmp.sock"
        launch = build_qemu_launch(args.arch, args.profile, environment)
        launch.argv[0] = qemu
        launch.argv.extend(["-qmp", f"unix:{qmp_socket},server=on,wait=off"])
        process = subprocess.Popen(launch.argv, cwd=SCRIPTS.parent, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        injected, injection_message = inject_qmp_events(qmp_socket, min(args.timeout, 4.0))
        try:
            try:
                output, _ = process.communicate(timeout=args.timeout)
            except subprocess.TimeoutExpired:
                process.terminate()
                output, _ = process.communicate(timeout=5)
        finally:
            launch.cleanup()
    serial = output.decode("utf-8", errors="replace")
    missing = missing_markers(serial)
    if missing:
        print("QEMU guest input smoke failed; missing markers:", file=sys.stderr)
        for marker in missing:
            print(f"  - {marker}", file=sys.stderr)
        print("--- QEMU serial tail ---", file=sys.stderr)
        print(serial[-8000:], file=sys.stderr)
        return 1
    if not injected:
        print(f"QEMU guest input smoke failed; {injection_message}", file=sys.stderr)
        print("--- QEMU serial tail ---", file=sys.stderr)
        print(serial[-8000:], file=sys.stderr)
        return 1
    print(f"QEMU guest input smoke passed arch={args.arch} devices=2")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
