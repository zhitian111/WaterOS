#!/usr/bin/env python3
"""Verify a small QEMU guest reaches block/devfs/rootfs bring-up markers."""

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
    "[driver] devices registered:",
    "[fs::devfs] refresh done",
    "[fs::rootfs] mount root RW from /dev/vda1",
    "entered runner",
)


def diagnose_inputs(arch: str, kernel: Path, sdcard: Path) -> list[str]:
    errors: list[str] = []
    if arch not in QEMU_BINARIES:
        errors.append(f"unsupported architecture: {arch}")
    for label, path in (("kernel", kernel), ("sdcard", sdcard)):
        if not path.is_file():
            errors.append(f"{label} not found: {path}")
        elif path.stat().st_size == 0:
            errors.append(f"{label} is empty: {path}")
    return errors


def missing_markers(serial: str) -> list[str]:
    return [marker for marker in REQUIRED_MARKERS if marker not in serial]


def run_guest(arch: str, profile: str, kernel: Path, sdcard: Path, timeout: float) -> str:
    environment = dict(os.environ)
    environment.update({
        "WOS_KERNEL": str(kernel.resolve()),
        "WOS_SDCARD": str(sdcard.resolve()),
        "WOS_SMP": "1",
        "WOS_QEMU_SNAPSHOT": "1",
        "WOS_QEMU_DISPLAY": "none",
    })
    launch = build_qemu_launch(arch, profile, environment)
    launch.argv[0] = shutil.which(QEMU_BINARIES[arch]) or launch.argv[0]
    process = subprocess.Popen(launch.argv,
                               cwd=SCRIPTS.parent,
                               stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT)
    try:
        try:
            output, _ = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            process.terminate()
            output, _ = process.communicate(timeout=5)
    finally:
        launch.cleanup()
    return output.decode("utf-8", errors="replace")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=tuple(QEMU_BINARIES), default="rv")
    parser.add_argument("--profile", choices=("pre", "final"), default="final")
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--sdcard", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=12.0)
    args = parser.parse_args()
    errors = diagnose_inputs(args.arch, args.kernel, args.sdcard)
    if shutil.which(QEMU_BINARIES.get(args.arch, "")) is None:
        errors.append(f"QEMU binary not found: {QEMU_BINARIES.get(args.arch, args.arch)}")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    if errors:
        for error in errors:
            print(f"SKIP: {error}", file=sys.stderr)
        return 77
    serial = run_guest(args.arch, args.profile, args.kernel, args.sdcard, args.timeout)
    missing = missing_markers(serial)
    if missing:
        print("QEMU rootfs guest smoke failed; missing markers:", file=sys.stderr)
        for marker in missing:
            print(f"  - {marker}", file=sys.stderr)
        print(serial[-8000:], file=sys.stderr)
        return 1
    print(f"QEMU rootfs guest smoke passed arch={args.arch} markers={len(REQUIRED_MARKERS)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
