#!/usr/bin/env python3
"""Launch a built WaterOS kernel and verify its TCP monitor end to end."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))

from qemu_run import build_qemu_launch
from remote_debug_client import connect_with_retry, run_smoke


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=("rv", "la"), default="rv")
    parser.add_argument("--profile", choices=("pre", "final"), default="pre")
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--sdcard", type=Path, required=True)
    parser.add_argument("--port", type=int, default=22323)
    parser.add_argument("--smp", type=int, choices=range(1, 9), default=1)
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()
    if not args.kernel.is_file():
        parser.error(f"kernel not found: {args.kernel}")
    if not args.sdcard.is_file():
        parser.error(f"sdcard not found: {args.sdcard}")

    environment = dict(os.environ)
    environment.update({
        "WOS_KERNEL": str(args.kernel.resolve()),
        "WOS_SDCARD": str(args.sdcard.resolve()),
        "WOS_SMP": str(args.smp),
        "WOS_QEMU_SNAPSHOT": "1",
        "WOS_REMOTE_DEBUG_PORT": str(args.port),
    })
    launch = build_qemu_launch(args.arch, args.profile, environment)
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
    print(f"remote-debug QEMU smoke passed arch={args.arch} smp={args.smp} snapshot=1")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
