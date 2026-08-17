#!/usr/bin/env python3
"""WaterOS QEMU 命令的唯一组装入口。

Make、兼容 Shell 脚本与 GDB 工具都复用本模块，避免架构、
profile 和磁盘策略在多份脚本中漂移。
"""
from __future__ import annotations

import argparse
import os
import platform
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))
from source.argparse_utils import ChineseArgumentParser  # noqa: E402


OS_ROOT = Path(__file__).resolve().parents[2]
VALID_ARCHES = {"rv", "la"}
VALID_PROFILES = {"pre", "final"}
DISPLAY_BACKEND_PREFERENCES = {
    "Darwin": ["cocoa", "sdl", "gtk", "none"],
}


class QemuConfigError(ValueError):
    """QEMU 配置无效。"""


@dataclass
class QemuLaunch:
    """一次可启动的 QEMU 配置。"""

    argv: list[str]

    def cleanup(self) -> None:
        pass


def _value(environment: Mapping[str, str], name: str, default: str = "") -> str:
    return environment.get(name, default).strip()


def _validate_token(name: str, value: str) -> None:
    if any(character.isspace() for character in value):
        raise QemuConfigError(f"{name} 不能包含空白字符: {value!r}")


def _supported_display_backends(arch: str) -> set[str]:
    qemu_binary = "qemu-system-riscv64" if arch == "rv" else "qemu-system-loongarch64"
    try:
        result = subprocess.run(
            [qemu_binary, "-display", "help"],
            cwd=OS_ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=False,
        )
    except OSError:
        return {"none"}

    supported: set[str] = set()
    capture = False
    for line in result.stdout.splitlines():
        stripped = line.strip()
        if not stripped:
            if capture:
                break
            continue
        if stripped == "Available display backend types:":
            capture = True
            continue
        if capture:
            supported.add(stripped.split(",", 1)[0])
    return supported or {"none"}


def _choose_display_backend(arch: str, requested: str) -> str:
    supported = _supported_display_backends(arch)
    if requested and requested != "auto":
        _validate_token("WOS_QEMU_DISPLAY", requested)
        if requested not in supported:
            raise QemuConfigError(
                f"WOS_QEMU_DISPLAY={requested!r} 不受当前 QEMU 支持，可用值: {', '.join(sorted(supported))}"
            )
        return requested

    preferences = DISPLAY_BACKEND_PREFERENCES.get(platform.system(), ["gtk", "sdl", "cocoa", "none"])
    for backend in preferences:
        if backend in supported:
            return backend
    return "none"


def build_qemu_launch(
    arch: str,
    profile: str,
    environment: Mapping[str, str] | None = None,
    *,
    root: Path = OS_ROOT,
) -> QemuLaunch:
    """根据架构、阶段和环境变量生成 QEMU argv。"""

    if arch not in VALID_ARCHES:
        raise QemuConfigError(f"ARCH 必须是 rv/la，当前为 {arch!r}")
    if profile not in VALID_PROFILES:
        raise QemuConfigError(f"PROFILE 必须是 pre/final，当前为 {profile!r}")
    env = os.environ if environment is None else environment
    root = root.resolve()
    smp = _value(env, "WOS_SMP", "8")
    try:
        smp_number = int(smp)
    except ValueError as exc:
        raise QemuConfigError(f"WOS_SMP 必须是 1..8，当前为 {smp!r}") from exc
    if not 1 <= smp_number <= 8:
        raise QemuConfigError(f"WOS_SMP 必须是 1..8，当前为 {smp!r}")
    memory = _value(env, "WOS_QEMU_MEM") or ("1G" if profile == "pre" else "8G")
    kernel_value = _value(env, "WOS_KERNEL") or f"./kernel-{arch}-{profile}"
    kernel = Path(kernel_value)
    if not kernel.is_absolute():
        kernel = root / kernel

    sdcard_value = _value(env, "WOS_SDCARD")
    if not sdcard_value:
        raise QemuConfigError(
            "未指定根文件系统镜像；请通过 Makefile 的 *_IMAGE 或 SDCARD 传入"
        )
    sdcard = Path(sdcard_value)
    if not sdcard.is_absolute():
        sdcard = root / sdcard

    graphics_value = _value(env, "WOS_GRAPHICS", "0")
    if graphics_value not in {"0", "1"}:
        raise QemuConfigError("WOS_GRAPHICS 必须是 0 或 1")
    graphics = graphics_value == "1"
    display_backend = _choose_display_backend(arch, _value(env, "WOS_QEMU_DISPLAY", "auto"))
    console_args = (
        ["-display", display_backend, "-serial", "stdio", "-monitor", "none"]
        if graphics
        else ["-nographic"]
    )

    if arch == "rv":
        drive_options = _value(env, "WOS_QEMU_IMAGE_DRIVE_OPTIONS")
        drive_spec = f"file={sdcard},if=none,format=raw,id=x0"
        if drive_options:
            drive_spec += f",{drive_options}"
        netdev_net = "user,id=net"
        hostfwd = _value(env, "WOS_QEMU_HOSTFWD", "tcp:127.0.0.1:2222-:22")
        if hostfwd:
            netdev_net += f",hostfwd={hostfwd}"
        argv = [
            "qemu-system-riscv64", "-machine", "virt", "-kernel", str(kernel),
            "-m", memory, *console_args, "-smp", smp, "-bios", "default",
            "-drive", drive_spec,
            "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
            "-no-reboot", "-device", "virtio-net-device,netdev=net",
            "-netdev", netdev_net, "-rtc", "base=utc",
        ]
        if graphics:
            argv.extend([
                "-device", "virtio-gpu-device",
                "-device", "virtio-keyboard-device",
                "-device", "virtio-tablet-device",
            ])
    else:
        netdev_net = "user,id=net0"
        hostfwd = _value(env, "WOS_QEMU_HOSTFWD", "tcp:127.0.0.1:2222-:22")
        if hostfwd:
            netdev_net += f",hostfwd={hostfwd}"
        argv = [
            "qemu-system-loongarch64", "-kernel", str(kernel), "-m", memory,
            *console_args, "-smp", smp,
            "-drive", f"file={sdcard},if=none,format=raw,id=x0",
            "-device", "virtio-blk-pci,drive=x0", "-no-reboot",
            "-device", "virtio-net-pci,netdev=net0", "-netdev", netdev_net,
            "-rtc", "base=utc",
        ]
        if graphics:
            argv.extend([
                "-device", "virtio-gpu-pci",
                "-device", "virtio-keyboard-pci",
                "-device", "virtio-tablet-pci",
            ])

    snapshot = _value(env, "WOS_QEMU_SNAPSHOT", "0") == "1"
    if snapshot:
        argv.append("-snapshot")

    gdb_enabled = _value(env, "WOS_QEMU_GDB", "0") == "1"
    gdb_wait = _value(env, "WOS_QEMU_GDB_WAIT", "0") == "1"
    if gdb_enabled or gdb_wait:
        port = _value(env, "WOS_QEMU_GDB_PORT", "1234")
        try:
            port_number = int(port)
        except ValueError as exc:
            raise QemuConfigError(f"GDB port 无效: {port!r}") from exc
        if not 1 <= port_number <= 65535:
            raise QemuConfigError(f"GDB port 必须是 1..65535: {port_number}")
        argv.extend(["-gdb", f"tcp:127.0.0.1:{port_number}"])
    if gdb_wait:
        argv.append("-S")
    cpuset = _value(env, "WOS_TASKSET_CPUS")
    if cpuset:
        _validate_token("WOS_TASKSET_CPUS", cpuset)
        argv = ["taskset", "-c", cpuset, *argv]
    return QemuLaunch(argv)


def main() -> int:
    parser = ChineseArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""环境变量:
  WOS_SDCARD                 根文件系统镜像路径，必须设置
  WOS_KERNEL                 内核路径，默认 kernel-<arch>-<profile>
  WOS_SMP                    Guest CPU 数量，默认为 8，范围为 1 至 8
  WOS_QEMU_MEM               Guest 内存大小，pre 默认 1G，final 默认 8G
  WOS_QEMU_SNAPSHOT          设为 1 时启用 QEMU snapshot
  WOS_GRAPHICS               设为 1 时启用图形设备
  WOS_QEMU_DISPLAY           图形后端，默认为 auto
  WOS_QEMU_GDB               设为 1 时开启 GDB server
  WOS_QEMU_GDB_WAIT          设为 1 时等待 GDB 连接后启动
  WOS_QEMU_GDB_PORT          GDB 端口，默认为 1234
  WOS_TASKSET_CPUS           绑定的宿主 CPU 列表，例如 0-3
  WOS_QEMU_IMAGE_DRIVE_OPTIONS  追加到 RISC-V 镜像 drive 的选项
  WOS_QEMU_HOSTFWD           追加到 user netdev 的 hostfwd 规则，默认
                             tcp:127.0.0.1:2222-:22（host 2222 端口转发到
                             guest 22 端口，供 SSH 使用；设为空串禁用）
""",
    )
    parser.add_argument(
        "--arch",
        choices=sorted(VALID_ARCHES),
        required=True,
        help="Guest 架构：rv 为 RISC-V64，la 为 LoongArch64",
    )
    parser.add_argument(
        "--profile",
        choices=sorted(VALID_PROFILES),
        required=True,
        help="比赛阶段：pre 为初赛，final 为决赛在线环境",
    )
    args = parser.parse_args()
    try:
        launch = build_qemu_launch(args.arch, args.profile)
    except QemuConfigError as exc:
        parser.error(str(exc))
    try:
        return subprocess.call(launch.argv, cwd=OS_ROOT)
    finally:
        launch.cleanup()


if __name__ == "__main__":
    raise SystemExit(main())
