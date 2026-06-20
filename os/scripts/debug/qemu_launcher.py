#!/usr/bin/env python3
"""Build QEMU argv; PC trace via ``-D /dev/fd/N`` (see pc_trace_watch.py)."""
from __future__ import annotations

import shutil
from pathlib import Path
from typing import Literal

Arch = Literal["rv", "la"]


def _qemu_bin(arch: Arch) -> list[str]:
    name = "qemu-system-riscv64" if arch == "rv" else "qemu-system-loongarch64"
    return [name]


def build_qemu_trace_cmd(arch: Arch, work_dir: Path, trace_fd: int | None = None) -> list[str]:
    """Build QEMU argv.

    When ``trace_fd`` is set, add ``-D /dev/fd/N`` so exec trace bypasses stdout.
    Serial / kernel log stays on stdout only; pass ``trace_fd`` via ``pass_fds``.
    """
    work_dir = work_dir.resolve()
    if arch == "rv":
        kernel = work_dir / "kernel-rv"
        sdcard = work_dir / "sdcard-rv.img"
        cmd: list[str] = [
            *_qemu_bin("rv"),
            "-machine",
            "virt",
            "-kernel",
            str(kernel),
            "-m",
            "1G",
            "-nographic",
            "-smp",
            "1",
            "-bios",
            "default",
            "-drive",
            f"file={sdcard},if=none,format=raw,id=x0",
            "-device",
            "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
            "-no-reboot",
            "-device",
            "virtio-net-device,netdev=net",
            "-netdev",
            "user,id=net",
            "-rtc",
            "base=utc",
        ]
    else:
        kernel = work_dir / "kernel-la"
        sdcard = work_dir / "sdcard-la.img"
        cmd = [
            *_qemu_bin("la"),
            "-kernel",
            str(kernel),
            "-m",
            "1G",
            "-nographic",
            "-smp",
            "1",
            "-drive",
            f"file={sdcard},if=none,format=raw,id=x0",
            "-device",
            "virtio-blk-pci,drive=x0",
            "-no-reboot",
            "-device",
            "virtio-net-pci,netdev=net0",
            "-netdev",
            "user,id=net0",
            "-rtc",
            "base=utc",
        ]

    cmd.extend(["-d", "exec,nochain"])
    if trace_fd is not None:
        cmd.extend(["-D", f"/dev/fd/{trace_fd}"])
    return cmd
