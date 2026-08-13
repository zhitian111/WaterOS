#!/usr/bin/env python3
"""Build the fixed mGBA core and the WaterOS Nano-X frontend statically."""

from __future__ import annotations

import argparse
import json
import lzma
import os
import shutil
import subprocess
from pathlib import Path


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
    if machine.lower() not in header.lower() or "INTERP" in program or "NEEDED" in dynamic:
        raise RuntimeError("water-mgba is not a static ELF for the requested target")


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
    user_root = Path(context["user_root"])
    cross = context["cross_compile"]
    cflags = context["cflags"]
    env = os.environ.copy()
    env["SOURCE_DATE_EPOCH"] = context["source_date_epoch"]
    for inherited in ("MAKEFLAGS", "MFLAGS", "MAKEOVERRIDES"):
        env.pop(inherited, None)

    if not (work / "CMakeLists.txt").is_file():
        raise RuntimeError(
            "vendored mGBA source is not initialized; from the repository root run: "
            "git submodule update --init --recursive user/vendor/mgba"
        )

    build = work / "wateros-build"
    run(["cmake", "-S", str(work), "-B", str(build),
         "-DCMAKE_SYSTEM_NAME=Linux", f"-DCMAKE_C_COMPILER={cross}gcc",
         "-DCMAKE_POLICY_VERSION_MINIMUM=3.5",
         "-DCMAKE_BUILD_TYPE=Release", "-DLIBMGBA_ONLY=ON", "-DM_CORE_GBA=ON",
         "-DM_CORE_GB=OFF", "-DBUILD_STATIC=ON", "-DBUILD_SHARED=OFF"],
        cwd=work, env=env)
    run(["cmake", "--build", str(build), f"-j{context['jobs']}"], cwd=work, env=env)

    # The Nano-X package installs runtime binaries, while this frontend needs its static
    # client archive at link time. Build the same fixed source/config in isolated work.
    nano_work = build / "microwindows"
    shutil.copytree(user_root / "vendor/microwindows", nano_work, symlinks=True)
    for patch in sorted((user_root / "packages/microwindows/patches").glob("*.patch")):
        run(["patch", "-p1", "-i", str(patch)], cwd=nano_work, env=env)
    nano_src = nano_work / "src"
    shutil.copy2(user_root / "packages/microwindows/config/wateros", nano_src / "config")
    nano_extra = "-DWATEROS_NANOX=1"
    if (context["arch"] == "rv" and is_arch_linux() and
            Path("/usr/riscv64-linux-gnu/include/linux/fb.h").is_file()):
        nano_extra += " -isystem /usr/riscv64-linux-gnu/include"
    nano_make = ["make", f"-j{context['jobs']}", "ARCH=LINUX-NATIVE",
                 f"NATIVETOOLSPREFIX={cross}", "LDFLAGS=-static",
                 f"EXTRAFLAGS={nano_extra}"]
    run([*nano_make, str(nano_src / "bin"), str(nano_src / "lib"), "subdirs"],
        cwd=nano_src, env=env)

    output = build / "water-mgba"
    run([f"{cross}gcc", "-std=c11", "-D_POSIX_C_SOURCE=200809L", "-O2", "-static", *cflags,
         "-I", str(work / "include"), "-I", str(build / "include"),
         "-I", str(nano_src / "include"), str(package / "wateros/main.c"),
         str(build / "libmgba.a"), str(nano_src / "lib/libnano-X.a"),
         "-lm", "-lpthread", "-o", str(output)], cwd=work, env=env)

    installed = destdir / "usr/bin/water-mgba"
    installed.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(output, installed)
    installed.chmod(0o755)
    validate_static(installed, context["readelf"], context["elf_machine"])

    rom = package / "roms/pokemon_phatom5.0.gba.xz"
    if not rom.is_file():
        raise RuntimeError(f"missing mGBA test ROM: {rom}")
    games = destdir / "games"
    games.mkdir(parents=True, exist_ok=True)
    installed_rom = games / rom.with_suffix("").name
    with lzma.open(rom, "rb") as source, installed_rom.open("wb") as target:
        shutil.copyfileobj(source, target)
    installed_rom.chmod(0o644)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
