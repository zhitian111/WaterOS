#!/usr/bin/env python3
"""Safely copy a verified WaterOS raw image to an SD-card or disk target."""

from __future__ import annotations

import argparse
import array
import fcntl
import os
import shutil
import stat
import subprocess
from pathlib import Path

from root_image import ImageError, verify_image


class FlashError(RuntimeError):
    """The source or explicit target safety contract failed."""


BLKGETSIZE64 = 0x80081272


def target_capacity_bytes(target: Path, target_stat: os.stat_result) -> int:
    if stat.S_ISREG(target_stat.st_mode):
        return target_stat.st_size
    if not stat.S_ISBLK(target_stat.st_mode):
        raise FlashError("cannot determine target capacity")
    capacity = array.array("Q", [0])
    try:
        with target.open("rb", buffering=0) as handle:
            fcntl.ioctl(handle.fileno(), BLKGETSIZE64, capacity, True)
    except OSError as error:
        raise FlashError(f"cannot query block-device capacity for {target}: {error}") from error
    return capacity[0]


def mounted_block_paths(target: Path, runner=subprocess.run) -> list[str]:
    """Return target/partition names with non-empty mountpoints."""
    try:
        result = runner(["lsblk", "-nrpo", "NAME,MOUNTPOINT", str(target)],
                        check=True, text=True, stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE)
    except FileNotFoundError as error:
        raise FlashError("lsblk is required to check mounted block devices") from error
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or "").strip()
        raise FlashError(f"lsblk failed while checking {target}: {detail}") from error
    mounted: list[str] = []
    for line in result.stdout.splitlines():
        fields = line.split(maxsplit=1)
        if len(fields) == 2 and fields[1].strip() not in ("", "-"):
            mounted.append(fields[0])
    return mounted


def validate_target(source: Path, target: Path, *, allow_regular_file: bool,
                    confirmed: bool, dry_run: bool) -> int:
    if not source.is_file():
        raise FlashError(f"source image not found: {source}")
    if source.resolve() == target.resolve():
        raise FlashError("source and target must be different")
    try:
        target_stat = target.stat()
    except OSError as error:
        raise FlashError(f"cannot inspect target {target}: {error}") from error
    is_block = stat.S_ISBLK(target_stat.st_mode)
    if not is_block and not (allow_regular_file and stat.S_ISREG(target_stat.st_mode)):
        raise FlashError("target must be a block device (or an explicitly allowed regular file)")
    if is_block and not confirmed and not dry_run:
        raise FlashError("writing a block device requires --yes-i-really-mean-it")
    if is_block:
        mounted = mounted_block_paths(target)
        if mounted:
            raise FlashError("target or partition is mounted: " + ", ".join(mounted))
    source_size = source.stat().st_size
    if source_size == 0:
        raise FlashError("source image is empty")
    target_size = target_capacity_bytes(target, target_stat)
    if target_size < source_size:
        raise FlashError(f"target is smaller than source ({target_size} < {source_size})")
    return source_size


def flash(source: Path, target: Path, *, allow_regular_file: bool = False,
          confirmed: bool = False, dry_run: bool = False) -> int:
    size = validate_target(source, target, allow_regular_file=allow_regular_file,
                           confirmed=confirmed, dry_run=dry_run)
    if dry_run:
        return size
    with source.open("rb") as src, target.open("r+b", buffering=0) as dst:
        shutil.copyfileobj(src, dst, length=1024 * 1024)
        dst.flush()
        os.fsync(dst.fileno())
    return size


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--target", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--allow-regular-file", action="store_true",
                        help="test-only/file-backed target; real SD targets should be /dev nodes")
    parser.add_argument("--yes-i-really-mean-it", action="store_true",
                        help="required before writing a block device")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    try:
        verify_image(args.image, args.manifest)
        size = flash(args.image, args.target,
                     allow_regular_file=args.allow_regular_file,
                     confirmed=args.yes_i_really_mean_it,
                     dry_run=args.dry_run)
    except (ImageError, FlashError, OSError) as error:
        parser.error(str(error))
    action = "would write" if args.dry_run else "wrote"
    print(f"{action} {size} bytes from {args.image} to {args.target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
