#!/usr/bin/env python3
"""Install the architecture-independent WaterOS root filesystem skeleton."""

from __future__ import annotations

import argparse
import json
import os
import shutil
from pathlib import Path


DIRECTORIES = (
    "bin", "sbin", "usr/bin", "usr/sbin", "etc", "etc/init.d",
    "etc/profile.d",
    "etc/wateros", "root", "home", "tmp", "run", "var", "var/log",
    "var/tmp", "dev", "dev/shm", "proc", "sys", "mnt",
    "opt/wateros/bin", "var/lib/wateros",
)
SPECIAL_MODES = {
    "root": 0o700,
    "tmp": 0o1777,
    "var/tmp": 0o1777,
    "dev/shm": 0o1777,
}


def copy_skeleton(source: Path, destination: Path) -> None:
    for directory in DIRECTORIES:
        (destination / directory).mkdir(parents=True, exist_ok=True)
    for directory, mode in SPECIAL_MODES.items():
        (destination / directory).chmod(mode)
    for entry in sorted(source.rglob("*")):
        relative = entry.relative_to(source)
        target = destination / relative
        if entry.is_dir():
            target.mkdir(parents=True, exist_ok=True)
        elif entry.is_symlink():
            target.parent.mkdir(parents=True, exist_ok=True)
            target.symlink_to(os.readlink(entry))
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(entry, target)
    var_run = destination / "var/run"
    if not var_run.exists():
        var_run.symlink_to("../run")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True, type=Path)
    args = parser.parse_args()
    context = json.loads(args.context.read_text(encoding="utf-8"))
    copy_skeleton(Path(context["user_root"]) / "rootfs/base",
                  Path(context["destdir"]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
