#!/usr/bin/env python3
"""WaterOS QEMU 命令的唯一组装入口。

Make、兼容 shell 脚本与 GDB 工具都复用本模块，避免架构、
profile、bootargs 和磁盘策略在多份脚本中漂移。
"""
from __future__ import annotations

import argparse
import os
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping


OS_ROOT = Path(__file__).resolve().parent.parent
VALID_ARCHES = {"rv", "la"}
VALID_PROFILES = {"pre", "final"}
VALID_MODES = {"auto", "shell", "run"}
VALID_TTYS = {"interactive", "closed", "fixture"}
VALID_ON_EXIT = {"shutdown", "shell", "reboot"}
VALID_LOGS = {"error", "warn", "info", "debug", "trace"}


class QemuConfigError(ValueError):
    """QEMU 或 WaterOS bootargs 配置无效。"""


@dataclass
class QemuLaunch:
    """一次可启动的 QEMU 配置及其临时文件。"""

    argv: list[str]
    temporary_files: list[Path]

    def cleanup(self) -> None:
        for path in self.temporary_files:
            try:
                path.unlink()
            except FileNotFoundError:
                pass


def _value(environment: Mapping[str, str], name: str, default: str = "") -> str:
    return environment.get(name, default).strip()


def _validate_token(name: str, value: str) -> None:
    if any(character.isspace() for character in value):
        raise QemuConfigError(f"{name} 不能包含空白字符: {value!r}")


def _bootargs(arch: str, environment: Mapping[str, str]) -> tuple[str, str]:
    mode = _value(environment, "WOS_MODE", "auto")
    if mode not in VALID_MODES:
        raise QemuConfigError(f"WOS_MODE 必须是 auto/shell/run，当前为 {mode!r}")

    script = _value(environment, "WOS_SCRIPT")
    if mode == "run":
        if not script:
            raise QemuConfigError("WOS_MODE=run 时必须提供 WOS_SCRIPT")
        if not script.startswith("/"):
            raise QemuConfigError("WOS_SCRIPT 必须是 guest 内的绝对路径")
    elif script:
        raise QemuConfigError("WOS_SCRIPT 只能与 WOS_MODE=run 一起使用")

    smp = _value(environment, "WOS_SMP", "8")
    try:
        smp_number = int(smp)
    except ValueError as exc:
        raise QemuConfigError(f"WOS_SMP 必须是 1..8，当前为 {smp!r}") from exc
    if not 1 <= smp_number <= 8:
        raise QemuConfigError(f"WOS_SMP 必须是 1..8，当前为 {smp!r}")

    optional = {
        "wos.shell": _value(environment, "WOS_SHELL"),
        "wos.script": script,
        "wos.on_exit": _value(environment, "WOS_ON_EXIT"),
        "wos.tty": _value(environment, "WOS_TTY"),
        "wos.log": _value(environment, "WOS_LOG"),
    }
    if optional["wos.on_exit"] and optional["wos.on_exit"] not in VALID_ON_EXIT:
        raise QemuConfigError("WOS_ON_EXIT 必须是 shutdown/shell/reboot")
    if optional["wos.tty"] and optional["wos.tty"] not in VALID_TTYS:
        raise QemuConfigError("WOS_TTY 必须是 interactive/closed/fixture")
    if optional["wos.log"] and optional["wos.log"] not in VALID_LOGS:
        raise QemuConfigError("WOS_LOG 必须是 error/warn/info/debug/trace")

    fields = [f"wos.mode={mode}"]
    if arch == "la":
        fields.append(f"wos.cpus={smp_number}")
    for key, value in optional.items():
        if value:
            _validate_token(key, value)
            fields.append(f"{key}={value}")
    return " ".join(fields), str(smp_number)


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
    bootargs, smp = _bootargs(arch, env)
    memory = "1G" if profile == "pre" else "8G"
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

    if arch == "rv":
        argv = [
            "qemu-system-riscv64", "-machine", "virt", "-kernel", str(kernel),
            "-m", memory, "-nographic", "-smp", smp, "-bios", "default",
            "-drive", f"file={sdcard},if=none,format=raw,id=x0",
            "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
            "-no-reboot", "-device", "virtio-net-device,netdev=net",
            "-netdev", "user,id=net", "-rtc", "base=utc",
        ]
    else:
        argv = [
            "qemu-system-loongarch64", "-kernel", str(kernel), "-m", memory,
            "-nographic", "-smp", smp,
            "-drive", f"file={sdcard},if=none,format=raw,id=x0",
            "-device", "virtio-blk-pci,drive=x0", "-no-reboot",
            "-device", "virtio-net-pci,netdev=net0", "-netdev", "user,id=net0",
            "-rtc", "base=utc",
        ]

    argv.extend(["-append", bootargs])
    temporary_files: list[Path] = []
    if arch == "la":
        # LoongArch direct ELF boot 会清空 argc/argv/envp，因此同时将
        # bootargs 放入 platform::boot 约定的 early-RAM mailbox。
        with tempfile.NamedTemporaryFile(
            prefix="wateros-la-bootargs.", delete=False
        ) as stream:
            stream.write(b"WOSCMD1" + bootargs.encode("utf-8") + b"\0")
            mailbox = Path(stream.name)
        temporary_files.append(mailbox)
        argv.extend(["-device", f"loader,file={mailbox},addr=0xa0000000,force-raw=on"])

    snapshot = _value(env, "WOS_QEMU_SNAPSHOT", "0") == "1"
    write_disk = _value(env, "WOS_WRITE_DISK", "0") == "1"
    mode = _value(env, "WOS_MODE", "auto")
    if snapshot or (mode != "auto" and not write_disk):
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
    return QemuLaunch(argv, temporary_files)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=sorted(VALID_ARCHES), required=True)
    parser.add_argument("--profile", choices=sorted(VALID_PROFILES), required=True)
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
