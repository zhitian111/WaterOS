#!/usr/bin/env python3
"""Cross-build and validate one statically linked BusyBox."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path


REQUIRED_APPLETS = ("cat", "ip", "ls", "mount", "nc", "ping", "ps", "umount",
                    "vi", "wget")


def run(command: list[str], *, cwd: Path, env: dict[str, str]) -> None:
    print("[userland]", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def validate_elf(binary: Path, readelf: str, expected_machine: str) -> None:
    header = subprocess.run([readelf, "-h", str(binary)], check=True,
                            text=True, capture_output=True).stdout
    program = subprocess.run([readelf, "-l", str(binary)], check=True,
                             text=True, capture_output=True).stdout
    dynamic = subprocess.run([readelf, "-d", str(binary)], check=True,
                             text=True, capture_output=True).stdout
    if expected_machine.lower() not in header.lower():
        raise RuntimeError(f"unexpected ELF machine; expected {expected_machine!r}")
    if "INTERP" in program or "NEEDED" in dynamic:
        raise RuntimeError("BusyBox is dynamically linked")


def validate_applets(destdir: Path) -> None:
    search_roots = ("bin", "sbin", "usr/bin", "usr/sbin")
    for applet in REQUIRED_APPLETS:
        candidates = [destdir / directory / applet for directory in search_roots]
        installed = next((candidate for candidate in candidates
                          if candidate.exists() or candidate.is_symlink()), None)
        if installed is None:
            raise RuntimeError(f"BusyBox install is missing required applet {applet!r}")
        if not installed.is_symlink():
            raise RuntimeError(f"BusyBox applet is not a symbolic link: {installed}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True, type=Path)
    args = parser.parse_args()
    context = json.loads(args.context.read_text(encoding="utf-8"))
    work = Path(context["work_dir"])
    package = Path(context["package_dir"])
    destdir = Path(context["destdir"])
    cross = context["cross_compile"]
    arch = context["kernel_arch"]
    jobs = str(context["jobs"])
    env = os.environ.copy()
    env["KCONFIG_NOTIMESTAMP"] = "1"
    env["SOURCE_DATE_EPOCH"] = context["source_date_epoch"]
    env["KBUILD_BUILD_TIMESTAMP"] = context["source_date_epoch"]
    shutil.copy2(package / "config/wateros_defconfig", work / ".config")
    if context["arch"] == "la":
        # Ubuntu 的 LoongArch UAPI 已删除旧 CBQ traffic-control 结构，而
        # BusyBox 1.33 的 `tc` applet 仍直接引用它们。Nano-X/rootfs 不依赖
        # 该 applet；仅在 LA 工作副本中关闭，保留 tcpsvd 和通用网络工具。
        config = work / ".config"
        text = config.read_text(encoding="utf-8")
        text = text.replace("CONFIG_TC=y", "# CONFIG_TC is not set")
        text = text.replace("CONFIG_FEATURE_TC_INGRESS=y",
                            "# CONFIG_FEATURE_TC_INGRESS is not set")
        config.write_text(text, encoding="utf-8")

    # BusyBox consumes CONFIG_EXTRA_CFLAGS in Makefile.flags.  Passing it as a
    # make command-line variable keeps the vendored source/config immutable and
    # ensures the architecture ABI selected in architectures.toml is honored.
    extra_cflags = " ".join(context["cflags"])
    common = ["make", f"-j{jobs}", f"ARCH={arch}", f"CROSS_COMPILE={cross}",
              f"CONFIG_EXTRA_CFLAGS={extra_cflags}"]
    # BusyBox 1.33 uses the older Kconfig target name.  `silentoldconfig`
    # resolves newly introduced symbols without turning a reproducible build
    # into an interactive prompt (and unlike Linux's `olddefconfig`, this
    # target actually exists in the vendored release).
    run(common + ["silentoldconfig"], cwd=work, env=env)
    run(common, cwd=work, env=env)
    run(common + [f"CONFIG_PREFIX={destdir}", "install"], cwd=work, env=env)

    busybox = destdir / "bin/busybox"
    if not busybox.is_file():
        raise RuntimeError("BusyBox install did not produce /bin/busybox")
    shell = destdir / "bin/sh"
    if not shell.exists() and not shell.is_symlink():
        shell.symlink_to("busybox")
    if not shell.is_symlink():
        raise RuntimeError("/bin/sh must be a symbolic link to BusyBox")
    validate_applets(destdir)
    readelf = context["readelf"]
    validate_elf(busybox, readelf, context["elf_machine"])
    strip = shutil.which(f"{cross}strip")
    if strip:
        run([strip, "--strip-unneeded", str(busybox)], cwd=work, env=env)
        validate_elf(busybox, readelf, context["elf_machine"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
