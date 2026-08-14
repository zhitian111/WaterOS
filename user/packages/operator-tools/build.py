#!/usr/bin/env python3
"""Install small shell-only tools used during on-site diagnosis."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True, type=Path)
    args = parser.parse_args()
    context = json.loads(args.context.read_text(encoding="utf-8"))
    package = Path(context["package_dir"])
    destination = Path(context["destdir"]) / "opt/wateros/bin"
    destination.mkdir(parents=True, exist_ok=True)
    for script in sorted((package / "scripts").iterdir()):
        target = destination / script.name
        shutil.copy2(script, target)
        target.chmod(0o755)
    cross = context["cross_compile"]
    smoke_source = package / "src/syscall-transfer-smoke.c"
    smoke_target = destination / "wos-syscall-smoke"
    env = os.environ.copy()
    subprocess.run([f"{cross}gcc", "-std=c11", "-O2", "-static",
                    *context["cflags"], str(smoke_source), "-o", str(smoke_target)],
                   env=env, check=True)
    smoke_target.chmod(0o755)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
