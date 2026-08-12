#!/usr/bin/env python3
"""组装 QEMU 参数；通过 ``-D /dev/fd/N`` 输出 PC trace。"""
from __future__ import annotations

import shutil
from pathlib import Path
from typing import Literal

Arch = Literal["rv", "la"]


def _qemu_bin(arch: Arch) -> list[str]:
    name = "qemu-system-riscv64" if arch == "rv" else "qemu-system-loongarch64"
    return [name]


def build_qemu_trace_cmd(arch: Arch, work_dir: Path, trace_fd: int | None = None) -> list[str]:
    """组装 QEMU argv。

    设置 ``trace_fd`` 时追加 ``-D /dev/fd/N``，让 exec trace 绕过 stdout；
    串口与内核日志仍只写 stdout，调用者通过 ``pass_fds`` 传递该描述符。
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
