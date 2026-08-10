#!/usr/bin/env python3
"""Verify QEMU virtio keyboard/tablet enumeration inside a WaterOS guest."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
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
    launch = build_qemu_launch(args.arch, args.profile, environment)
    launch.argv[0] = qemu
    process = subprocess.Popen(
        launch.argv,
        cwd=SCRIPTS.parent,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
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
    print(f"QEMU guest input smoke passed arch={args.arch} devices=2")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
