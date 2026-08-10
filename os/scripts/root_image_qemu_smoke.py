#!/usr/bin/env python3
"""Build a tiny root image and ask QEMU's image tool to inspect it."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS / "root_image"))

from root_image import build_image, manifest_file_contents, verify_image


def inspect_with_qemu_img(image: Path, qemu_img: str = "qemu-img") -> dict[str, object]:
    result = subprocess.run(
        [qemu_img, "info", "--output=json", str(image)],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    info = json.loads(result.stdout)
    if info.get("format") != "raw":
        raise RuntimeError(f"QEMU reported unexpected image format: {info!r}")
    return info


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--partition-table", choices=("mbr", "gpt"), default="gpt")
    parser.add_argument("--size-mib", type=int, default=16)
    parser.add_argument("--manifest", type=Path, default=SCRIPTS / "root_image" / "rootfs-manifest.json")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    qemu_img = shutil.which("qemu-img")
    if qemu_img is None:
        print("SKIP: 未找到 qemu-img")
        return 77
    if args.size_mib < 16:
        parser.error("--size-mib must be at least 16")
    temporary: tempfile.TemporaryDirectory[str] | None = None
    if args.output is None:
        temporary = tempfile.TemporaryDirectory(prefix="wateros-root-qemu-")
        output = Path(temporary.name) / "root.img"
    else:
        output = args.output.resolve()
    try:
        namespace = argparse.Namespace(
            output=output,
            manifest=args.manifest,
            size_mib=args.size_mib,
            start_sector=2048,
            partition_table=args.partition_table,
            uuid="574f5300-0000-4000-8000-000000000001",
            label="WATEROS_ROOT",
            force=True,
        )
        required = build_image(namespace)
        verify_image(output, required, manifest_file_contents(args.manifest))
        info = inspect_with_qemu_img(output, qemu_img)
        expected_bytes = args.size_mib * 1024 * 1024
        if info.get("virtual-size") != expected_bytes:
            raise RuntimeError(f"QEMU size mismatch: {info.get('virtual-size')} != {expected_bytes}")
        print(f"QEMU image smoke passed table={args.partition_table} size={expected_bytes}")
        return 0
    finally:
        if temporary is not None:
            temporary.cleanup()


if __name__ == "__main__":
    raise SystemExit(main())
