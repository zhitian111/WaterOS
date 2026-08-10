#!/usr/bin/env python3
"""Launch a built WaterOS kernel and verify its TCP monitor end to end."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))

from qemu_run import build_qemu_launch
from remote_debug_client import connect_with_retry, run_smoke


QEMU_BINARIES = {"rv": "qemu-system-riscv64", "la": "qemu-system-loongarch64"}
MONITOR_LISTEN_MARKER = "[remote-debug] unauthenticated development monitor listening"


def qemu_binary_name(arch: str) -> str:
    return QEMU_BINARIES[arch]


def diagnose_inputs(arch: str, kernel: Path, sdcard: Path, port: int) -> list[str]:
    """Return actionable preflight errors without launching QEMU."""
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
    if not 1 <= port <= 65535:
        errors.append(f"port must be in 1..65535: {port}")
    return errors


def monitor_listening_seen(serial_tail: str) -> bool:
    """Whether the guest reached the monitor listen loop.

    This separates a missing compile-time feature from a later guest network
    forwarding/RX/TCP failure when a QEMU connection attempt times out.
    """
    return MONITOR_LISTEN_MARKER in serial_tail


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=("rv", "la"), default="rv")
    parser.add_argument("--profile", choices=("pre", "final"), default="pre")
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--sdcard", type=Path, required=True)
    parser.add_argument("--port", type=int, default=22323)
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()
    errors = diagnose_inputs(args.arch, args.kernel, args.sdcard, args.port)
    if errors:
        print("SKIP: QEMU guest 产物/参数尚未准备好:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 77
    qemu = shutil.which(qemu_binary_name(args.arch))
    if qemu is None:
        print(f"SKIP: 未找到 {qemu_binary_name(args.arch)}", file=sys.stderr)
        return 77

    environment = dict(os.environ)
    environment.update({
        "WOS_KERNEL": str(args.kernel.resolve()),
        "WOS_SDCARD": str(args.sdcard.resolve()),
        "WOS_SMP": "1",
        "WOS_QEMU_SNAPSHOT": "1",
        "WOS_REMOTE_DEBUG_PORT": str(args.port),
    })
    launch = build_qemu_launch(args.arch, args.profile, environment)
    launch.argv[0] = qemu
    with tempfile.TemporaryFile() as serial_log:
        process = subprocess.Popen(
            launch.argv,
            cwd=SCRIPTS.parent,
            stdin=subprocess.DEVNULL,
            stdout=serial_log,
            stderr=subprocess.STDOUT,
        )
        try:
            client = connect_with_retry("127.0.0.1", args.port, args.timeout)
            try:
                results = run_smoke(client)
            finally:
                client.close()
        except BaseException:
            serial_log.seek(0)
            tail = serial_log.read().decode("utf-8", errors="replace")[-8000:]
            print("--- QEMU serial tail ---", file=sys.stderr)
            print(tail, file=sys.stderr)
            if monitor_listening_seen(tail):
                print(
                    "DIAGNOSIS: guest monitor reached listen state, but the host "
                    "forwarded TCP connection failed; inspect virtio-net RX/TCP "
                    "polling rather than the monitor feature gate.",
                    file=sys.stderr,
                )
            else:
                print(
                    "DIAGNOSIS: guest monitor listen marker was not observed; "
                    "check the kernel remote-debug-monitor feature/profile.",
                    file=sys.stderr,
                )
            raise
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            launch.cleanup()

    for result in results:
        print(f"[{result.command}] {result.response.rstrip()}")
    print(f"remote-debug QEMU smoke passed arch={args.arch} snapshot=1")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
