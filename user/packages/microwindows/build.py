#!/usr/bin/env python3
"""为 WaterOS 交叉构建并安装静态 Nano-X、演示程序和 Doom。"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path


NANOX_BINARIES = ("nano-X", "nxlaunch", "nxclock", "nxeyes", "nxcalc", "nxedit", "nxev")
DOOM_BINARY = "doom"
DOOM_WAD = "doom1.wad"


def run(command: list[str], *, cwd: Path, env: dict[str, str]) -> None:
    print("[userland]", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def validate_static(binary: Path, readelf: str, machine: str) -> None:
    header = subprocess.run([readelf, "-h", str(binary)], check=True,
                            text=True, capture_output=True).stdout
    program = subprocess.run([readelf, "-l", str(binary)], check=True,
                             text=True, capture_output=True).stdout
    dynamic = subprocess.run([readelf, "-d", str(binary)], check=True,
                             text=True, capture_output=True).stdout
    if machine.lower() not in header.lower():
        raise RuntimeError(f"unexpected Nano-X ELF machine; expected {machine!r}")
    if "INTERP" in program or "NEEDED" in dynamic:
        raise RuntimeError(f"{binary.name} is not statically linked")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True, type=Path)
    args = parser.parse_args()
    context = json.loads(args.context.read_text(encoding="utf-8"))
    work = Path(context["work_dir"])
    package = Path(context["package_dir"])
    destdir = Path(context["destdir"])
    src = work / "src"
    config = src / "config"
    shutil.copy2(package / "config/wateros", config)

    env = os.environ.copy()
    # `make -C user ... ARCH=rv` 会通过 MAKEFLAGS 把 rv 继续注入嵌套 make，
    # 覆盖 Microwindows 自己表示 Linux ABI 的 ARCH=LINUX-NATIVE。
    for inherited in ("MAKEFLAGS", "MFLAGS", "MAKEOVERRIDES"):
        env.pop(inherited, None)
    env["SOURCE_DATE_EPOCH"] = context["source_date_epoch"]
    cross = context["cross_compile"]
    extra = " ".join(context["cflags"])
    # Microwindows' Makefile.rules clears config-file LDFLAGS, therefore the
    # static-link requirement must be a make command-line assignment.
    command = ["make", f"-j{context['jobs']}", "ARCH=LINUX-NATIVE",
               f"NATIVETOOLSPREFIX={cross}",
               "LDFLAGS=-static", f"EXTRAFLAGS=-DWATEROS_NANOX=1 {extra}"]
    # `all` 会递归进入与 Nano-X 无关的 Win32/Nuklear/游戏目录；先构建核心和
    # server，再只进入 Nano-X demo 目录，缩短离线交叉编译时间并减少 syscall 需求。
    run([*command, "default"], cwd=src, env=env)
    run([*command, "-C", "demos/nanox", "all"], cwd=src, env=env)

    # contrib/doom 使用的是较老的 C89 风格源码，不能跟随现代 GCC 默认的
    # gnu17 方言。musl 也不声明历史 BSD 宏 IPPORT_USERRESERVED，因此在此
    # 固定其兼容值。Doom 最终仍静态链接到上面生成的 libnano-X.a。
    doom_cc = " ".join((f"{cross}gcc", "-std=gnu89",
                        "-DIPPORT_USERRESERVED=5000", extra))
    run(["make", f"-j{context['jobs']}", "-C", "contrib/doom",
         f"CC={doom_cc}", "LDFLAGS=-static", "all"], cwd=src, env=env)

    destination = destdir / "usr/bin"
    destination.mkdir(parents=True, exist_ok=True)
    installed_binaries: list[Path] = []
    for name in NANOX_BINARIES:
        source = src / "bin" / name
        if not source.is_file():
            raise RuntimeError(f"Microwindows build did not produce {name}")
        target = destination / name
        shutil.copy2(source, target)
        target.chmod(0o755)
        validate_static(target, context["readelf"], context["elf_machine"])
        installed_binaries.append(target)

    doom_source = src / "contrib/doom" / DOOM_BINARY
    if not doom_source.is_file():
        raise RuntimeError("Microwindows build did not produce Doom")
    installed_doom = destination / DOOM_BINARY
    shutil.copy2(doom_source, installed_doom)
    installed_doom.chmod(0o755)
    validate_static(installed_doom, context["readelf"], context["elf_machine"])
    installed_binaries.append(installed_doom)

    strip = shutil.which(f"{cross}strip")
    if strip:
        run([strip, "--strip-unneeded", *map(str, installed_binaries)],
            cwd=work, env=env)

    for script_name in ("start-nanox", "start-doom"):
        launcher = package / "scripts" / script_name
        installed_launcher = destdir / "usr/bin" / script_name
        shutil.copy2(launcher, installed_launcher)
        installed_launcher.chmod(0o755)

    # WAD 是运行时游戏数据，不链接进 ELF，方便以后替换为用户合法持有的
    # doom.wad/doom2.wad。当前镜像安装仓库中已经验证可用的 doom1.wad。
    wad_source = src / "contrib/doom" / DOOM_WAD
    if not wad_source.is_file():
        raise RuntimeError(f"missing Doom data file: {wad_source}")
    with wad_source.open("rb") as stream:
        wad_magic = stream.read(4)
    if wad_magic not in (b"IWAD", b"PWAD"):
        raise RuntimeError(f"invalid {DOOM_WAD}: expected IWAD/PWAD header")
    wad_directory = destdir / "usr/share/games/doom"
    wad_directory.mkdir(parents=True, exist_ok=True)
    installed_wad = wad_directory / DOOM_WAD
    shutil.copy2(wad_source, installed_wad)
    installed_wad.chmod(0o644)

    configuration = destdir / "etc/wateros/nxlaunch.cnf"
    configuration.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(package / "config/nxlaunch.cnf", configuration)
    configuration.chmod(0o644)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
