#!/usr/bin/env python3
"""为 WaterOS 双架构交叉构建并安装静态 Nano-X、演示程序和 Doom。"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


NANOX_BINARIES = ("nano-X", "nxlaunch", "nxclock", "nxeyes", "nxcalc", "nxedit", "nxev",
                  "nxterm",)
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


def is_arch_linux() -> bool:
    try:
        return "ID=arch" in Path("/etc/os-release").read_text(encoding="utf-8")
    except OSError:
        return False


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
    # Microwindows' Makefile.rules clears config-file LDFLAGS, therefore the
    # static-link requirement must be a make command-line assignment.
    nano_extra = "-DWATEROS_NANOX=1"
    if (context["arch"] == "rv" and is_arch_linux() and
            Path("/usr/riscv64-linux-gnu/include/linux/fb.h").is_file()):
        nano_extra += " -isystem /usr/riscv64-linux-gnu/include"
    command = ["make", f"-j{context['jobs']}", "ARCH=LINUX-NATIVE",
               f"NATIVETOOLSPREFIX={cross}",
               "LDFLAGS=-static", f"EXTRAFLAGS={nano_extra}"]
    # The top-level `default` target appends an empty `LIBNAME` as `/lib/` in
    # this upstream snapshot, which GNU make treats as an unbuildable target.
    # Build its meaningful prerequisites explicitly: output directories plus
    # the core/server subdirectories. This keeps the vendor source untouched.
    run([*command, str(src / "bin"), str(src / "lib"), "subdirs"], cwd=src, env=env)
    run([*command, "-C", "demos/nanox", "all"], cwd=src, env=env)

    # contrib/doom 使用的是较老的 C89 风格源码，不能跟随现代 GCC 默认的
    # gnu17 方言。RISC-V 的 musl 头文件不声明历史 BSD 宏
    # IPPORT_USERRESERVED，需要补上兼容值；LoongArch 使用的 glibc 头文件
    # 已经声明了同名枚举，重复以宏定义会破坏 <netinet/in.h> 的语法。
    # Doom 最终仍静态链接到上面生成的 libnano-X.a。
    doom_flags = ["-std=gnu89"]
    if context["arch"] == "rv":
        doom_flags.append("-DIPPORT_USERRESERVED=5000")
    doom_cc = " ".join((f"{cross}gcc", *doom_flags, *context["cflags"]))
    # Doom 随源码附带的 Makefile/configure 已经是可直接使用的生成产物。
    # 这些文件来自很老的 Automake 1.4；源码包中的 configure.in 时间戳偶尔
    # 会比 Makefile.in 新几个毫秒，使现代 GNU Make 误判为需要重新运行宿主机
    # automake。那既会引入不必要的宿主依赖，也会因缺少 compile/depcomp 而
    # 失败。用 -o 将 Autotools 生成物声明为固定输入，只执行真正的 C 编译。
    run(["make", f"-j{context['jobs']}", "-C", "contrib/doom",
         "-o", "Makefile", "-o", "Makefile.in", "-o", "configure",
         "-o", "aclocal.m4", f"CC={doom_cc}", "LDFLAGS=-static", "all"],
        cwd=src, env=env)

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

    # 独立于图形服务验证 /dev/ptmx、/dev/pts/N、setsid 和控制终端取得流程。
    installed_pty_smoke = destination / "pty-smoke"
    run([f"{cross}gcc", "-static", *context["cflags"],
         str(package / "scripts/pty-smoke.c"), "-o", str(installed_pty_smoke)],
        cwd=work, env=env)
    installed_pty_smoke.chmod(0o755)
    validate_static(installed_pty_smoke, context["readelf"], context["elf_machine"])
    installed_binaries.append(installed_pty_smoke)

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

    # Nano-X 只启用了无外部依赖的 PNM 解码器。构建时将版本库中的 PNG
    # 背景转换为固定分辨率 PPM，并生成与启动器主题一致的小图标，避免把
    # Pillow/ImageMagick 等宿主工具变成用户空间构建依赖。
    run([sys.executable, str(package / "tools/prepare_assets.py"),
         "--source", str(package / "assets/wateros-waves.png"),
         "--output", str(destdir / "usr/share/wateros")], cwd=work, env=env)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
