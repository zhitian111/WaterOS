#!/usr/bin/env python3
"""Build the small WaterOS Nano-X file manager as a static user program."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path


def run(command: list[str], *, cwd: Path, env: dict[str, str]) -> None:
    print("[userland]", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


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
    user_root = Path(context["user_root"])
    destdir = Path(context["destdir"])
    cross = context["cross_compile"]
    env = os.environ.copy()
    env["SOURCE_DATE_EPOCH"] = context["source_date_epoch"]
    for inherited in ("MAKEFLAGS", "MFLAGS", "MAKEOVERRIDES"):
        env.pop(inherited, None)

    nano_work = work / "microwindows"
    shutil.copytree(user_root / "vendor/microwindows", nano_work, symlinks=True)
    for patch in sorted((user_root / "packages/microwindows/patches").glob("*.patch")):
        run(["patch", "-p1", "-i", str(patch)], cwd=nano_work, env=env)
    nano_src = nano_work / "src"
    shutil.copy2(user_root / "packages/microwindows/config/wateros", nano_src / "config")
    extra = "-DWATEROS_NANOX=1"
    if (context["arch"] == "rv" and is_arch_linux() and
            Path("/usr/riscv64-linux-gnu/include/linux/fb.h").is_file()):
        extra += " -isystem /usr/riscv64-linux-gnu/include"
    make = ["make", f"-j{context['jobs']}", "ARCH=LINUX-NATIVE",
            f"NATIVETOOLSPREFIX={cross}", "LDFLAGS=-static", f"EXTRAFLAGS={extra}"]
    run([*make, str(nano_src / "bin"), str(nano_src / "lib"), "subdirs"],
        cwd=nano_src, env=env)

    output = work / "waterfm"
    run([f"{cross}gcc", "-std=c11", "-D_POSIX_C_SOURCE=200809L", "-O2", "-static",
         *context["cflags"], "-I", str(nano_src / "include"),
         str(package / "wateros/main.c"), str(nano_src / "lib/libnano-X.a"),
         "-o", str(output)], cwd=work, env=env)
    installed = destdir / "usr/bin/waterfm"
    installed.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(output, installed)
    installed.chmod(0o755)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
