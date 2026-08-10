#!/usr/bin/env python3
"""Validate and optionally run a small physical-root image under QEMU."""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from qemu_run import QemuConfigError, build_qemu_launch  # noqa: E402
from root_image import ImageError, manifest_file_contents, manifest_paths, verify_image  # noqa: E402


class SmokeError(RuntimeError):
    """The image or launch contract cannot be executed."""


@dataclass(frozen=True)
class RootMountEvidence:
    """The strongest root-mount signal found in a kernel log."""

    state: str
    line: str | None = None


_ROOT_MOUNT_SUCCESS_MARKERS = (
    "[bringup][stage-00-bus] ext4 root mounted (RW)",
    "[fs::rootfs] mount root RW from /dev/vda1",
    "[fs::rootfs] mount default root RO from /dev/vda1",
)
_ROOT_MOUNT_FAILURE_MARKERS = (
    "[bringup][stage-00-bus] root mount failed",
    "[fs] init: no root block device available",
    "[fs] init: lookup block device",
)
_AUX_MOUNT_SUCCESS_MARKER = "[bringup][stage-00-bus] aux ext4 mounted"
_AUX_MOUNT_FAILURE_MARKER = "[bringup][stage-00-bus] aux mount failed"


def parse_root_mount_evidence(output: str) -> RootMountEvidence:
    """Classify root-mount evidence, preferring a later successful mount."""

    failure: str | None = None
    for line in output.splitlines():
        if any(marker in line for marker in _ROOT_MOUNT_SUCCESS_MARKERS):
            return RootMountEvidence("success", line)
        if failure is None and any(marker in line for marker in _ROOT_MOUNT_FAILURE_MARKERS):
            failure = line
    if failure is not None:
        return RootMountEvidence("failure", failure)
    return RootMountEvidence("absent")


def parse_aux_mount_evidence(output: str) -> RootMountEvidence:
    """Classify optional aux mount evidence from the bring-up log."""
    failure: str | None = None
    for line in output.splitlines():
        if _AUX_MOUNT_SUCCESS_MARKER in line:
            return RootMountEvidence("success", line)
        if failure is None and _AUX_MOUNT_FAILURE_MARKER in line:
            failure = line
    if failure is not None:
        return RootMountEvidence("failure", failure)
    return RootMountEvidence("absent")


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
                # The LA virt bring-up needs the same 1 GiB floor used by the
                # maintained LA launcher; keep RV smoke small for disk tests.
                "WOS_QEMU_MEM": "1G" if arch == "la" else "256M",
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


def run_smoke(
    command: list[str], *, root: Path, timeout: float, require_root_mount: bool = False,
    require_aux_mount: bool = False,
) -> int:
    try:
        completed = subprocess.run(
            command,
            cwd=root,
            timeout=timeout,
            check=False,
            stdout=subprocess.PIPE if (require_root_mount or require_aux_mount) else None,
            stderr=subprocess.STDOUT if (require_root_mount or require_aux_mount) else None,
            text=require_root_mount or require_aux_mount,
        )
    except FileNotFoundError as error:
        raise SmokeError(f"QEMU binary not found: {command[0]}") from error
    except subprocess.TimeoutExpired as error:
        # A kernel normally keeps running after bring-up. In strict evidence
        # mode, a timeout is successful if the requested mount lines were
        # already emitted; subprocess has still terminated the child.
        partial = error.stdout or ""
        if isinstance(partial, bytes):
            partial = partial.decode(errors="replace")
        root_ok = (not require_root_mount or
                   parse_root_mount_evidence(partial).state == "success")
        aux_ok = (not require_aux_mount or
                  parse_aux_mount_evidence(partial).state == "success")
        if root_ok and aux_ok:
            print("root-image-qemu-smoke: evidence collected before timeout")
            return 0
        raise SmokeError(f"QEMU smoke timed out after {timeout:g}s") from error
    if completed.returncode != 0:
        return completed.returncode
    if require_root_mount:
        evidence = parse_root_mount_evidence(completed.stdout or "")
        if evidence.state != "success":
            raise SmokeError(
                "root mount evidence missing (state: "
                f"{evidence.state}; expected ext4 root mount)"
            )
        if evidence.line:
            print("root-image-qemu-smoke: root mount evidence:", evidence.line)
    if require_aux_mount:
        evidence = parse_aux_mount_evidence(completed.stdout or "")
        if evidence.state != "success":
            raise SmokeError(
                "aux mount evidence missing (state: "
                f"{evidence.state}; expected configured ext4 aux mount)"
            )
        if evidence.line:
            print("root-image-qemu-smoke: aux mount evidence:", evidence.line)
    return 0


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--arch", choices=("rv", "la"), required=True)
    result.add_argument("--profile", choices=("pre", "final"), default="pre")
    result.add_argument("--image", type=Path, required=True)
    result.add_argument("--manifest", type=Path, required=True)
    result.add_argument("--data-manifest", type=Path)
    result.add_argument("--kernel", type=Path, required=True)
    result.add_argument("--execute", action="store_true")
    result.add_argument(
        "--require-root-mount",
        action="store_true",
        help="require a successful root-mount log line after QEMU exits",
    )
    result.add_argument(
        "--require-aux-mount",
        action="store_true",
        help="require a successful configured aux-mount log line after QEMU exits",
    )
    result.add_argument("--timeout", type=float, default=30.0)
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    root = SCRIPTS.parent
    image = args.image.resolve()
    manifest = args.manifest.resolve()
    kernel = args.kernel.resolve()
    try:
        data_manifest = args.data_manifest.resolve() if args.data_manifest else None
        verify_image(
            image,
            manifest_paths(manifest),
            manifest_file_contents(manifest),
            manifest_paths(data_manifest) if data_manifest else None,
            manifest_file_contents(data_manifest) if data_manifest else None,
        )
        command = build_smoke_command(args.arch, args.profile, image, kernel, root=root)
        print("root-image-qemu-smoke:", shlex.join(command))
        if (args.require_root_mount or args.require_aux_mount) and not args.execute:
            raise SmokeError("mount evidence options require --execute")
        if args.execute:
            return run_smoke(
                command,
                root=root,
                timeout=args.timeout,
                require_root_mount=args.require_root_mount,
                require_aux_mount=args.require_aux_mount,
            )
    except (ImageError, SmokeError) as error:
        print(f"root-image-qemu-smoke: error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
