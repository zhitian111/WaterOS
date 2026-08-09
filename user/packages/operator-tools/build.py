#!/usr/bin/env python3
"""Install small shell-only tools used during on-site diagnosis."""

from __future__ import annotations

import argparse
import json
import shutil
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
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

