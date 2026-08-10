#!/usr/bin/env python3
"""Validate and optionally run a small physical-root image under QEMU."""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from qemu_run import QemuConfigError, build_qemu_launch  # noqa: E402
from root_image import ImageError, manifest_file_contents, manifest_paths, verify_image  # noqa: E402


class SmokeError(RuntimeError):
    """The image or launch contract cannot be executed."""


def build_smoke_command(
    arch: str, profile: str, image: Path, kernel: Path, *, root: Path
) -> list[str]:
    if not image.is_file():
        raise SmokeError(f"root image does not exist: {image}")
    if not kernel.is_file():
        raise SmokeError(f"kernel artifact does not exist: {kernel}")
    try:
        launch = build_qemu_launch(
            arch,
            profile,
            {
                "WOS_KERNEL": str(kernel),
                "WOS_SDCARD": str(image),
                "WOS_QEMU_SNAPSHOT": "1",
                "WOS_SMP": "1",
                "WOS_QEMU_MEM": "256M",
                "WOS_GRAPHICS": "0",
            },
            root=root,
        )
    except QemuConfigError as error:
        raise SmokeError(str(error)) from error
    if "-snapshot" not in launch.argv:
        raise SmokeError("QEMU launch lost mandatory -snapshot protection")
    if not any(str(image) in argument for argument in launch.argv):
        raise SmokeError("QEMU launch does not reference the requested root image")
    return launch.argv


def run_smoke(command: list[str], *, root: Path, timeout: float) -> int:
    try:
        completed = subprocess.run(command, cwd=root, timeout=timeout, check=False)
    except FileNotFoundError as error:
        raise SmokeError(f"QEMU binary not found: {command[0]}") from error
    except subprocess.TimeoutExpired as error:
        raise SmokeError(f"QEMU smoke timed out after {timeout:g}s") from error
    return completed.returncode


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--arch", choices=("rv", "la"), required=True)
    result.add_argument("--profile", choices=("pre", "final"), default="pre")
    result.add_argument("--image", type=Path, required=True)
    result.add_argument("--manifest", type=Path, required=True)
    result.add_argument("--kernel", type=Path, required=True)
    result.add_argument("--execute", action="store_true")
    result.add_argument("--timeout", type=float, default=30.0)
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    root = SCRIPTS.parent
    image = args.image.resolve()
    manifest = args.manifest.resolve()
    kernel = args.kernel.resolve()
    try:
        verify_image(image, manifest_paths(manifest), manifest_file_contents(manifest))
        command = build_smoke_command(args.arch, args.profile, image, kernel, root=root)
        print("root-image-qemu-smoke:", shlex.join(command))
        if args.execute:
            return run_smoke(command, root=root, timeout=args.timeout)
    except (ImageError, SmokeError) as error:
        print(f"root-image-qemu-smoke: error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
