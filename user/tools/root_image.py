#!/usr/bin/env python3
"""Build and verify a small WaterOS physical root disk image without root access.

This script is owned by the userland builder and lives under `user/tools/`.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Iterable

SECTOR_SIZE = 512
DEFAULT_IMAGE_MIB = 32
DEFAULT_START_SECTOR = 2048
DEFAULT_BLOCK_SIZE = 4096
DEFAULT_UUID = "574f5300-0000-4000-8000-000000000001"
DEFAULT_LABEL = "WATEROS_ROOT"
DEFAULT_DISK_ID = "0x574f5301"
DEFAULT_BOOT_SIZE_MIB = 64
VF2_LOADER1_SECTORS = 2 * 1024 * 1024 // SECTOR_SIZE
VF2_LOADER2_SECTORS = 4 * 1024 * 1024 // SECTOR_SIZE
MBR_SIGNATURE = b"\x55\xaa"
GPT_SIGNATURE = b"EFI PART"
LINUX_PARTITION_TYPE = 0x83
GPT_LINUX_TYPE_GUID = bytes.fromhex("af3dc60f838472478e793d69d8477de4")
GPT_TYPE_RE = re.compile(r"(?:[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}|[A-Za-z])$")
MBR_TYPE_RE = re.compile(r"[0-9A-Fa-f]{1,2}$")


class ImageError(RuntimeError):
    """A malformed manifest/image or failed host tool invocation."""


@dataclass(frozen=True)
class Partition:
    number: int
    partition_type: int
    start_sector: int
    sectors: int
    table: str = "mbr"

    @property
    def byte_offset(self) -> int:
        return self.start_sector * SECTOR_SIZE

    @property
    def byte_length(self) -> int:
        return self.sectors * SECTOR_SIZE


def run(command: list[str], *, input_text: str | None = None) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            command,
            input=input_text,
            text=True,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
    except FileNotFoundError as error:
        raise ImageError(f"required host tool not found: {command[0]}") from error
    except subprocess.CalledProcessError as error:
        output = error.stdout.strip() if error.stdout else "no diagnostic output"
        raise ImageError(f"command failed ({' '.join(command)}): {output}") from error


def checked_guest_path(raw: Any) -> PurePosixPath:
    if not isinstance(raw, str):
        raise ImageError("manifest path must be a string")
    path = PurePosixPath(raw)
    if not path.is_absolute() or path == PurePosixPath("/") or ".." in path.parts:
        raise ImageError(f"unsafe guest path: {raw!r}")
    # `[`/`]` 是 BusyBox 的合法 applet 名（`/usr/bin/[`），保留在允许字符集内。
    if re.fullmatch(r"/[A-Za-z0-9._+\[\]/-]+", raw) is None:
        raise ImageError(f"guest path contains unsupported characters: {raw!r}")
    return path


def parse_mode(raw: Any) -> int:
    if not isinstance(raw, str) or len(raw) != 4 or any(char not in "01234567" for char in raw):
        raise ImageError(f"mode must be four octal digits: {raw!r}")
    return int(raw, 8)


def load_manifest(path: Path) -> dict[str, Any]:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ImageError(f"cannot read manifest {path}: {error}") from error
    if not isinstance(manifest, dict):
        raise ImageError("manifest root must be an object")
    for key in ("directories", "files"):
        if not isinstance(manifest.get(key, []), list):
            raise ImageError(f"manifest {key!r} must be an array")
    return manifest


def staging_path(root: Path, guest: PurePosixPath) -> Path:
    return root.joinpath(*guest.parts[1:])


def resolve_manifest_source(manifest_path: Path,
                            source_value: str,
                            source_root: Path | None = None) -> Path:
    source = Path(source_value)
    if source_root is None:
        if not source.is_absolute():
            source = manifest_path.parent / source
        return source
    if source.is_absolute():
        raise ImageError("source must be relative when --source-root is used")
    root = source_root.resolve()
    candidate = (root / source).resolve()
    try:
        candidate.relative_to(root)
    except ValueError as error:
        raise ImageError(f"source escapes source root: {source_value!r}") from error
    return candidate


def populate_staging(manifest_path: Path,
                     staging: Path,
                     source_root: Path | None = None) -> list[str]:
    manifest = load_manifest(manifest_path)
    required_paths: list[str] = []
    for entry in manifest.get("directories", []):
        if not isinstance(entry, dict):
            raise ImageError("directory entry must be an object")
    directories = sorted(
        manifest.get("directories", []),
        key=lambda entry: len(checked_guest_path(entry.get("path")).parts),
    )
    for entry in directories:
        guest = checked_guest_path(entry.get("path"))
        mode = parse_mode(entry.get("mode", "0755"))
        destination = staging_path(staging, guest)
        destination.mkdir(parents=True, exist_ok=True)
        destination.chmod(mode)
        required_paths.append(str(guest))

    for entry in manifest.get("files", []):
        if not isinstance(entry, dict):
            raise ImageError("file entry must be an object")
        guest = checked_guest_path(entry.get("path"))
        mode = parse_mode(entry.get("mode", "0644"))
        destination = staging_path(staging, guest)
        destination.parent.mkdir(parents=True, exist_ok=True)
        has_content = "content" in entry
        has_source = "source" in entry
        if has_content == has_source:
            raise ImageError(f"file {guest} must specify exactly one of content/source")
        if has_source:
            source_value = entry["source"]
            if not isinstance(source_value, str):
                raise ImageError(f"source for {guest} must be a string")
            source = resolve_manifest_source(manifest_path, source_value, source_root)
            if not source.is_file():
                raise ImageError(f"manifest source does not exist: {source}")
            shutil.copyfile(source, destination)
        else:
            content = entry["content"]
            if not isinstance(content, str):
                raise ImageError(f"inline content for {guest} must be a string")
            destination.write_text(content, encoding="utf-8")
        destination.chmod(mode)
        required_paths.append(str(guest))
    return required_paths


def copy_staging_tree(source: Path, dest: Path) -> list[str]:
    """Copy an existing rootfs staging tree into `dest`, preserving modes/symlinks."""
    if not source.is_dir():
        raise ImageError(f"copy-tree source is not a directory: {source}")
    for entry in sorted(source.rglob("*")):
        relative = entry.relative_to(source)
        target = dest / relative
        if entry.is_dir():
            target.mkdir(parents=True, exist_ok=True)
            target.chmod(entry.stat().st_mode & 0o7777)
        elif entry.is_symlink():
            target.parent.mkdir(parents=True, exist_ok=True)
            target.symlink_to(os.readlink(entry))
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(entry, target)
            target.chmod(entry.stat().st_mode & 0o7777)
    return tree_guest_paths(source)


def tree_guest_paths(source: Path) -> list[str]:
    """List guest-absolute paths of files/symlinks in a staging tree (no copy)."""
    if not source.is_dir():
        raise ImageError(f"copy-tree source is not a directory: {source}")
    paths: list[str] = []
    for entry in sorted(source.rglob("*")):
        if entry.is_file() or entry.is_symlink():
            paths.append(f"/{entry.relative_to(source).as_posix()}")
    return paths


def parse_mbr_sector(sector: bytes, image_bytes: int) -> list[Partition]:
    if len(sector) != SECTOR_SIZE:
        raise ImageError("short MBR sector")
    if sector[510:512] != MBR_SIGNATURE:
        raise ImageError("missing MBR signature")
    partitions: list[Partition] = []
    for index in range(4):
        entry = sector[446 + index * 16 : 462 + index * 16]
        partition_type = entry[4]
        start, sectors = struct.unpack_from("<II", entry, 8)
        if partition_type == start == sectors == 0:
            continue
        if partition_type == 0 or start == 0 or sectors == 0:
            raise ImageError(f"invalid MBR partition entry {index + 1}")
        partition = Partition(index + 1, partition_type, start, sectors)
        if partition.byte_offset + partition.byte_length > image_bytes:
            raise ImageError(f"partition {index + 1} extends beyond image")
        partitions.append(partition)
    for index, left in enumerate(partitions):
        left_end = left.start_sector + left.sectors
        for right in partitions[index + 1 :]:
            right_end = right.start_sector + right.sectors
            if left.start_sector < right_end and right.start_sector < left_end:
                raise ImageError("overlapping MBR partitions")
    return partitions


def crc32(data: bytes) -> int:
    value = 0xFFFFFFFF
    for byte in data:
        value ^= byte
        for _ in range(8):
            value = (value >> 1) ^ 0xEDB88320 if value & 1 else value >> 1
    return (~value) & 0xFFFFFFFF


def _parse_gpt_at(image: Path, header_lba: int, total_sectors: int,
                  expected_backup_lba: int) -> list[Partition]:
    image_bytes = image.stat().st_size
    with image.open("rb") as source:
        source.seek(header_lba * SECTOR_SIZE)
        header = source.read(SECTOR_SIZE)
        if header[:8] != GPT_SIGNATURE or len(header) != SECTOR_SIZE:
            raise ImageError("missing GPT header")
        header_size = struct.unpack_from("<I", header, 12)[0]
        current_lba, backup_lba = struct.unpack_from("<QQ", header, 24)
        first_usable, last_usable = struct.unpack_from("<QQ", header, 40)
        entries_lba, entry_count, entry_size, entries_crc = struct.unpack_from(
            "<QIII", header, 72
        )
        if (not 92 <= header_size <= SECTOR_SIZE or current_lba != header_lba or
                backup_lba != expected_backup_lba):
            raise ImageError("invalid GPT header")
        if backup_lba >= total_sectors or first_usable > last_usable >= total_sectors:
            raise ImageError("GPT usable range is invalid")
        header_copy = bytearray(header[:header_size])
        expected_crc = struct.unpack_from("<I", header, 16)[0]
        header_copy[16:20] = b"\0\0\0\0"
        if crc32(bytes(header_copy)) != expected_crc:
            raise ImageError("GPT header CRC mismatch")
        if not 128 <= entry_size <= 1024 or entry_size % 8 or entry_count > 4096:
            raise ImageError("unsupported GPT entry layout")
        entry_bytes = entry_count * entry_size
        entry_sectors = (entry_bytes + SECTOR_SIZE - 1) // SECTOR_SIZE
        if entries_lba == 0 or entries_lba + entry_sectors > total_sectors:
            raise ImageError("GPT entry array extends beyond image")
        source.seek(entries_lba * SECTOR_SIZE)
        entries = source.read(entry_bytes)
        if len(entries) != entry_bytes or crc32(entries) != entries_crc:
            raise ImageError("GPT entry array CRC mismatch")
    partitions: list[Partition] = []
    for index in range(entry_count):
        entry = entries[index * entry_size : (index + 1) * entry_size]
        if entry[:16] == b"\0" * 16:
            continue
        start, end = struct.unpack_from("<QQ", entry, 32)
        if start < first_usable or start > end or end > last_usable:
            raise ImageError(f"GPT partition {index + 1} is out of range")
        if entry[:16] == GPT_LINUX_TYPE_GUID:
            partition_type = LINUX_PARTITION_TYPE
        else:
            partition_type = 0
        partition = Partition(index + 1, partition_type, start, end - start + 1, "gpt")
        if partition.byte_offset + partition.byte_length > image_bytes:
            raise ImageError(f"GPT partition {index + 1} extends beyond image")
        partitions.append(partition)
    for index, left in enumerate(partitions):
        for right in partitions[index + 1 :]:
            if left.start_sector <= right.start_sector + right.sectors - 1 and right.start_sector <= left.start_sector + left.sectors - 1:
                raise ImageError("overlapping GPT partitions")
    return partitions


def parse_gpt_image(image: Path) -> list[Partition]:
    total_sectors = image.stat().st_size // SECTOR_SIZE
    if total_sectors < 2:
        raise ImageError("image is too small for GPT")
    try:
        return _parse_gpt_at(image, 1, total_sectors, total_sectors - 1)
    except ImageError as primary_error:
        try:
            return _parse_gpt_at(image, total_sectors - 1, total_sectors, 1)
        except ImageError:
            raise primary_error


def read_partitions(image: Path) -> list[Partition]:
    try:
        image_bytes = image.stat().st_size
        with image.open("rb") as source:
            sector = source.read(SECTOR_SIZE)
    except OSError as error:
        raise ImageError(f"cannot read image {image}: {error}") from error
    if len(sector) == SECTOR_SIZE and sector[510:512] == MBR_SIGNATURE and sector[450] == 0xEE:
        return parse_gpt_image(image)
    return parse_mbr_sector(sector, image_bytes)


def make_partition_table(image: Path, image_bytes: int, start_sector: int, table: str = "mbr") -> Partition:
    total_sectors = image_bytes // SECTOR_SIZE
    sectors = total_sectors - start_sector - (34 if table == "gpt" else 0)
    sectors -= sectors % (DEFAULT_BLOCK_SIZE // SECTOR_SIZE)
    if sectors <= 0:
        raise ImageError("image is too small for requested partition start")
    if table == "mbr":
        specification = ("label: dos\n" f"label-id: {DEFAULT_DISK_ID}\n" "unit: sectors\n\n"
                         f"{start_sector},{sectors},83\n")
    elif table == "gpt":
        specification = ("label: gpt\n" f"first-lba: {start_sector}\n" "unit: sectors\n\n"
                         f"{start_sector},{sectors},L\n")
    else:
        raise ImageError(f"unsupported partition table: {table}")
    run(["sfdisk", "--quiet", str(image)], input_text=specification)
    partition = read_partitions(image)
    if len(partition) != 1:
        raise ImageError("partitioning tool did not create exactly one partition")
    return partition[0]


def _partition_type(value: str, table: str) -> str:
    value = value.strip()
    valid = GPT_TYPE_RE.fullmatch(value) if table == "gpt" else MBR_TYPE_RE.fullmatch(value)
    if valid is None:
        raise ImageError(f"invalid {table} partition type: {value!r}")
    return value


def make_partition_table_with_images(image: Path, image_bytes: int, root_size_mib: int,
                                     extra_images: list[Path], extra_types: list[str],
                                     table: str) -> list[Partition]:
    """Create P1 rootfs followed by one partition for each raw filesystem image."""
    if table not in ("mbr", "gpt"):
        raise ImageError(f"unsupported partition table: {table}")
    if table == "mbr" and len(extra_images) + 1 > 4:
        raise ImageError("MBR supports at most four partitions including the rootfs")
    if len(extra_types) not in (0, len(extra_images)):
        raise ImageError("filesystem partition type count must match filesystem image count")
    if root_size_mib < 16:
        raise ImageError("root filesystem size must be at least 16 MiB")

    total_sectors = image_bytes // SECTOR_SIZE
    start = DEFAULT_START_SECTOR
    alignment = DEFAULT_BLOCK_SIZE // SECTOR_SIZE
    root_sectors = root_size_mib * 1024 * 1024 // SECTOR_SIZE
    root_sectors -= root_sectors % alignment
    starts_and_sizes: list[tuple[int, int]] = [(start, root_sectors)]
    cursor = start + root_sectors
    for source in extra_images:
        size = source.stat().st_size
        sectors = (size + SECTOR_SIZE - 1) // SECTOR_SIZE
        sectors = ((sectors + alignment - 1) // alignment) * alignment
        if sectors <= 0:
            raise ImageError(f"filesystem image is empty: {source}")
        starts_and_sizes.append((cursor, sectors))
        cursor += sectors
    table_sectors = 34 if table == "gpt" else 0
    if cursor + table_sectors > total_sectors:
        raise ImageError("disk image is too small for rootfs and filesystem images")

    default_type = "L" if table == "gpt" else "83"
    types = extra_types or [default_type] * len(extra_images)
    types = [_partition_type(value, table) for value in types]
    lines = ["label: gpt" if table == "gpt" else "label: dos"]
    if table == "mbr":
        lines.append(f"label-id: {DEFAULT_DISK_ID}")
    lines += ["unit: sectors", ""]
    root_type = "L" if table == "gpt" else "83"
    for index, (partition_start, sectors) in enumerate(starts_and_sizes):
        partition_type = root_type if index == 0 else types[index - 1]
        lines.append(f"{partition_start},{sectors},{partition_type}")
    run(["sfdisk", "--quiet", str(image)], input_text="\n".join(lines) + "\n")
    partitions = read_partitions(image)
    if len(partitions) != len(starts_and_sizes):
        raise ImageError(f"partitioning tool created {len(partitions)} partitions, expected {len(starts_and_sizes)}")
    return partitions


def _validate_raw_filesystem_image(path: Path) -> None:
    if not path.is_file():
        raise ImageError(f"filesystem image does not exist: {path}")
    if path.stat().st_size == 0:
        raise ImageError(f"filesystem image is empty: {path}")
    with path.open("rb") as source:
        header = source.read(SECTOR_SIZE)
    if len(header) >= SECTOR_SIZE and header[510:512] == MBR_SIGNATURE:
        raise ImageError(f"filesystem image must be unpartitioned: {path}")
    if header[:8] == GPT_SIGNATURE:
        raise ImageError(f"filesystem image must be unpartitioned: {path}")


def write_partition_image(image: Path, partition: Partition, source: Path) -> None:
    source_size = source.stat().st_size
    if source_size > partition.byte_length:
        raise ImageError(f"filesystem image does not fit partition: {source}")
    with source.open("rb") as input_file, image.open("r+b") as output:
        output.seek(partition.byte_offset)
        remaining = source_size
        while remaining:
            chunk = input_file.read(min(1024 * 1024, remaining))
            if not chunk:
                raise ImageError(f"short read from filesystem image: {source}")
            output.write(chunk)
            remaining -= len(chunk)
        padding = partition.byte_length - source_size
        while padding:
            chunk = b"\0" * min(1024 * 1024, padding)
            output.write(chunk)
            padding -= len(chunk)


def make_partition_table_vf2(image: Path, image_bytes: int, boot_size_mib: int,
                             table: str = "gpt", extra_images: list[Path] | None = None,
                             extra_types: list[str] | None = None) -> list[Partition]:
    """VisionFive 2 layout: P1/P2 placeholders, P3 FAT boot, P4 rootfs, then extras.

    出厂 U-Boot 默认 bootpart=3 / rootpart=4，distro 路径从 P3 sysboot
    /extlinux/extlinux.conf，因此分区编号必须与官方镜像保持一致。
    """
    extra_images = extra_images or []
    extra_types = extra_types or []
    if table == "mbr" and len(extra_images) > 0:
        raise ImageError("VF2 MBR layout has no free primary partition for extra images")
    if len(extra_types) not in (0, len(extra_images)):
        raise ImageError("filesystem partition type count must match filesystem image count")
    total_sectors = image_bytes // SECTOR_SIZE
    start = DEFAULT_START_SECTOR
    loader1_start = start
    loader2_start = loader1_start + VF2_LOADER1_SECTORS
    boot_start = loader2_start + VF2_LOADER2_SECTORS
    boot_sectors = boot_size_mib * 1024 * 1024 // SECTOR_SIZE
    root_start = boot_start + boot_sectors
    alignment = DEFAULT_BLOCK_SIZE // SECTOR_SIZE
    extra_sectors: list[int] = []
    for source in extra_images:
        sectors = (source.stat().st_size + SECTOR_SIZE - 1) // SECTOR_SIZE
        sectors = ((sectors + alignment - 1) // alignment) * alignment
        extra_sectors.append(sectors)
    root_sectors = total_sectors - root_start - sum(extra_sectors) - (34 if table == "gpt" else 0)
    root_sectors -= root_sectors % alignment
    if boot_sectors <= 0 or root_sectors <= 0:
        raise ImageError("image is too small for VF2 boot + rootfs layout")
    extra_starts: list[int] = []
    cursor = root_start + root_sectors
    for sectors in extra_sectors:
        extra_starts.append(cursor)
        cursor += sectors
    extra_partition_types = extra_types or (["L"] if table == "gpt" else ["83"]) * len(extra_images)
    extra_partition_types = [_partition_type(value, table) for value in extra_partition_types]
    if table == "mbr":
        specification = (
            "label: dos\n" f"label-id: {DEFAULT_DISK_ID}\n" "unit: sectors\n\n"
            f"{loader1_start},{VF2_LOADER1_SECTORS},0c\n"
            f"{loader2_start},{VF2_LOADER2_SECTORS},0c\n"
            f"{boot_start},{boot_sectors},0c\n"
            f"{root_start},{root_sectors},83\n" +
            "".join(f"{start},{sectors},{partition_type}\n"
                    for start, sectors, partition_type in zip(
                        extra_starts, extra_sectors, extra_partition_types))
        )
    elif table == "gpt":
        specification = (
            "label: gpt\n" f"first-lba: {start}\n" "unit: sectors\n\n"
            f"{loader1_start},{VF2_LOADER1_SECTORS},L\n"
            f"{loader2_start},{VF2_LOADER2_SECTORS},L\n"
            f"{boot_start},{boot_sectors},EBD0A0A2-B9E5-4433-87C0-68B6B72699C7\n"
            f"{root_start},{root_sectors},L\n" +
            "".join(f"{start},{sectors},{partition_type}\n"
                    for start, sectors, partition_type in zip(
                        extra_starts, extra_sectors, extra_partition_types))
        )
    else:
        raise ImageError(f"unsupported partition table: {table}")
    run(["sfdisk", "--quiet", str(image)], input_text=specification)
    partitions = read_partitions(image)
    expected = 4 + len(extra_images)
    if len(partitions) != expected:
        raise ImageError(f"partitioning tool created {len(partitions)} partitions, expected {expected}")
    return partitions


def make_partition_table_boot_root(image: Path, image_bytes: int, boot_size_mib: int,
                                   table: str = "mbr",
                                   extra_images: list[Path] | None = None,
                                   extra_types: list[str] | None = None) -> list[Partition]:
    """Conventional U-Boot layout: P1 FAT boot, P2 ext4 rootfs, then extras."""
    extra_images = extra_images or []
    extra_types = extra_types or []
    if table == "mbr" and len(extra_images) > 2:
        raise ImageError("boot-root MBR layout supports at most two extra partitions")
    if len(extra_types) not in (0, len(extra_images)):
        raise ImageError("filesystem partition type count must match filesystem image count")
    total_sectors = image_bytes // SECTOR_SIZE
    boot_start = DEFAULT_START_SECTOR
    boot_sectors = boot_size_mib * 1024 * 1024 // SECTOR_SIZE
    root_start = boot_start + boot_sectors
    alignment = DEFAULT_BLOCK_SIZE // SECTOR_SIZE
    extra_sectors = [
        ((source.stat().st_size + DEFAULT_BLOCK_SIZE - 1) // DEFAULT_BLOCK_SIZE)
        * alignment
        for source in extra_images
    ]
    root_sectors = total_sectors - root_start - sum(extra_sectors)
    if table == "gpt":
        root_sectors -= 34
    root_sectors -= root_sectors % alignment
    if boot_sectors <= 0 or root_sectors <= 0:
        raise ImageError("image is too small for boot-root boot + rootfs layout")
    extra_starts: list[int] = []
    cursor = root_start + root_sectors
    for sectors in extra_sectors:
        extra_starts.append(cursor)
        cursor += sectors
    default_type = "L" if table == "gpt" else "83"
    partition_types = [_partition_type(value, table)
                       for value in (extra_types or [default_type] * len(extra_images))]
    if table == "mbr":
        lines = ["label: dos", f"label-id: {DEFAULT_DISK_ID}", "unit: sectors", "",
                 f"{boot_start},{boot_sectors},0c,*",
                 f"{root_start},{root_sectors},83"]
    elif table == "gpt":
        lines = ["label: gpt", f"first-lba: {boot_start}", "unit: sectors", "",
                 (f"{boot_start},{boot_sectors},"
                  "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"),
                 f"{root_start},{root_sectors},L"]
    else:
        raise ImageError(f"unsupported partition table: {table}")
    lines.extend(f"{start},{sectors},{partition_type}"
                 for start, sectors, partition_type in
                 zip(extra_starts, extra_sectors, partition_types))
    run(["sfdisk", "--quiet", str(image)], input_text="\n".join(lines) + "\n")
    partitions = read_partitions(image)
    expected = 2 + len(extra_images)
    if len(partitions) != expected:
        raise ImageError(f"partitioning tool created {len(partitions)} partitions, expected {expected}")
    return partitions


def build_boot_partition(image: Path, partition: Partition, boot_dir: Path) -> None:
    """用 `mkfs.vfat` + mtools 把 `boot_dir` 写进 FAT 启动分区。"""
    if not boot_dir.is_dir():
        raise ImageError(f"boot directory does not exist: {boot_dir}")
    # mkfs.vfat -C takes 1024-byte blocks, while partition geometry is tracked
    # in 512-byte sectors.
    blocks = partition.byte_length // 1024
    if blocks <= 0:
        raise ImageError("boot partition is too small")
    with tempfile.TemporaryDirectory(prefix="wateros-boot-") as temporary:
        boot_fs = Path(temporary) / "boot.fat"
        try:
            run(["mkfs.vfat", "-n", "WATEROS", "-C", str(boot_fs), str(blocks)])
        except FileNotFoundError as error:
            raise ImageError("required host tool not found: mkfs.vfat") from error
        for item in sorted(boot_dir.rglob("*")):
            if not item.is_file():
                continue
            relative = item.relative_to(boot_dir).as_posix()
            parent = PurePosixPath(relative).parent
            if parent != PurePosixPath("."):
                # 已存在的目录 mmd 会报错，忽略；真正的写入失败由 mcopy 暴露。
                try:
                    run(["mmd", "-i", str(boot_fs), f"::/{parent.as_posix()}"])
                except ImageError:
                    pass
            run(["mcopy", "-i", str(boot_fs), str(item), f"::/{relative}"])
        with boot_fs.open("rb") as source, image.open("r+b") as output:
            output.seek(partition.byte_offset)
            remaining = partition.byte_length
            while remaining:
                chunk = source.read(min(1024 * 1024, remaining))
                if not chunk:
                    raise ImageError("short read while writing boot partition")
                output.write(chunk)
                remaining -= len(chunk)


def verify_boot_partition(image: Path, partition: Partition, boot_dir: Path) -> None:
    """抽验 FAT 启动分区：每个文件存在且与源内容一致。"""
    files = sorted(item for item in boot_dir.rglob("*") if item.is_file())
    if not files:
        raise ImageError("boot directory contains no files")
    with tempfile.TemporaryDirectory(prefix="wateros-boot-verify-") as temporary:
        boot_fs = Path(temporary) / "boot.fat"
        copy_partition(image, partition, boot_fs)
        for item in files:
            relative = item.relative_to(boot_dir).as_posix()
            extracted = Path(temporary) / "check.bin"
            run(["mcopy", "-i", str(boot_fs), "-n", f"::{relative}", str(extracted)])
            if extracted.read_bytes() != item.read_bytes():
                raise ImageError(f"boot file content differs from source: {relative}")


def build_image(args: argparse.Namespace) -> list[str]:
    image = args.output.resolve()
    if image.exists() and not args.force:
        raise ImageError(f"output already exists (use --force): {image}")
    image_bytes = args.size_mib * 1024 * 1024
    if image_bytes % SECTOR_SIZE != 0 or args.size_mib < 16:
        raise ImageError("image size must be at least 16 MiB and sector aligned")
    image.parent.mkdir(parents=True, exist_ok=True)
    extra_images = [path.resolve() for path in getattr(args, "extra_images", [])]
    for extra_image in extra_images:
        _validate_raw_filesystem_image(extra_image)
    boot_dir = getattr(args, "boot_dir", None)
    temporary_image: Path | None = None
    try:
        with tempfile.TemporaryDirectory(prefix="wateros-root-staging-") as temporary:
            staging = Path(temporary)
            source_root = getattr(args, "source_root", None)
            copy_tree = getattr(args, "copy_tree", None)
            if copy_tree is not None:
                required_paths = copy_staging_tree(copy_tree.resolve(), staging)
            else:
                required_paths = populate_staging(args.manifest.resolve(), staging,
                                                  source_root.resolve() if source_root else None)
            descriptor, raw_path = tempfile.mkstemp(
                prefix=f".{image.name}.", suffix=".tmp", dir=image.parent
            )
            os.close(descriptor)
            temporary_image = Path(raw_path)
            with temporary_image.open("wb") as output:
                output.truncate(image_bytes)
            boot_dir = getattr(args, "boot_dir", None)
            table = getattr(args, "partition_table", "mbr")
            if boot_dir is not None:
                boot_layout = getattr(args, "boot_layout", "vf2")
                partition_builder = (make_partition_table_vf2 if boot_layout == "vf2"
                                     else make_partition_table_boot_root)
                partitions = partition_builder(
                    temporary_image, image_bytes,
                    getattr(args, "boot_size_mib", DEFAULT_BOOT_SIZE_MIB), table,
                    extra_images, list(getattr(args, "extra_partition_types", [])),
                )
                boot_number, root_number = (3, 4) if boot_layout == "vf2" else (1, 2)
                boot_partition = next((p for p in partitions if p.number == boot_number), None)
                root_partition = next((p for p in partitions if p.number == root_number), None)
                if boot_partition is None or root_partition is None:
                    raise ImageError(f"{boot_layout} layout is missing boot/root partitions")
                build_boot_partition(temporary_image, boot_partition, boot_dir.resolve())
                partition = root_partition
            else:
                if extra_images:
                    partitions = make_partition_table_with_images(
                        temporary_image, image_bytes,
                        getattr(args, "root_size_mib", None) or args.size_mib,
                        extra_images, list(getattr(args, "extra_partition_types", [])), table,
                    )
                    partition = partitions[0]
                else:
                    partition = make_partition_table(temporary_image, image_bytes, args.start_sector, table)
                    if partition.partition_type != LINUX_PARTITION_TYPE:
                        raise ImageError("root partition has unexpected MBR type")
            if partition.byte_length % DEFAULT_BLOCK_SIZE != 0:
                raise ImageError("partition size is not aligned to ext4 block size")
            blocks = partition.byte_length // DEFAULT_BLOCK_SIZE
            environment = os.environ.copy()
            environment.setdefault("E2FSPROGS_FAKE_TIME", "1704067200")
            command = [
                "mkfs.ext4", "-q", "-F", "-b", str(DEFAULT_BLOCK_SIZE),
                "-L", args.label, "-U", args.uuid,
                # another_ext4 currently requires the 64-bit group descriptor
                # layout even for this small volume. This is QEMU-tested but
                # still needs validation with each board's storage controller.
                "-O", "^has_journal",
                "-E", f"offset={partition.byte_offset},lazy_itable_init=0,lazy_journal_init=0",
                "-d", str(staging), str(temporary_image), str(blocks),
            ]
            try:
                subprocess.run(command, env=environment, check=True)
            except FileNotFoundError as error:
                raise ImageError("required host tool not found: mkfs.ext4") from error
            except subprocess.CalledProcessError as error:
                raise ImageError(f"mkfs.ext4 failed with status {error.returncode}") from error
            if extra_images:
                extra_start = ((4 if boot_layout == "vf2" else 2)
                               if boot_dir is not None else 1)
                for extra_image, extra_partition in zip(extra_images, partitions[extra_start:]):
                    write_partition_image(temporary_image, extra_partition, extra_image)
            # Do not replace a known-good image before the replacement passes
            # both filesystem and manifest validation.
            expected_files = None if copy_tree is not None else manifest_file_contents(
                args.manifest.resolve()
            )
            verify_image(temporary_image, required_paths, expected_files,
                         boot_dir=boot_dir, extra_images=extra_images,
                         boot_layout=getattr(args, "boot_layout", "vf2"))
            os.replace(temporary_image, image)
            temporary_image = None
            return required_paths
    finally:
        if temporary_image is not None:
            temporary_image.unlink(missing_ok=True)


def copy_partition(image: Path, partition: Partition, destination: Path) -> None:
    with image.open("rb") as source, destination.open("wb") as output:
        source.seek(partition.byte_offset)
        remaining = partition.byte_length
        while remaining:
            chunk = source.read(min(1024 * 1024, remaining))
            if not chunk:
                raise ImageError("short read while extracting partition")
            output.write(chunk)
            remaining -= len(chunk)


def debugfs_command(partition_image: Path, command: str) -> str:
    return run(["debugfs", "-R", command, str(partition_image)]).stdout


def verify_image(
    image: Path, required_paths: Iterable[str],
    expected_files: dict[str, bytes] | None = None,
    boot_dir: Path | None = None,
    extra_images: list[Path] | None = None,
    boot_layout: str = "vf2",
) -> Partition:
    partitions = read_partitions(image)
    extra_images = extra_images or []
    if boot_dir is not None:
        base_count = 4 if boot_layout == "vf2" else 2
        expected_count = base_count + len(extra_images)
        if len(partitions) != expected_count:
            raise ImageError(f"expected {boot_layout} layout with {expected_count} partitions, found {len(partitions)}")
        boot_index, root_index = (2, 3) if boot_layout == "vf2" else (0, 1)
        boot_partition = partitions[boot_index]
        root_partition = partitions[root_index]
        extra_partitions = partitions[base_count:]
    else:
        expected_count = 1 + len(extra_images)
        if len(partitions) != expected_count:
            raise ImageError(f"expected {expected_count} partitions, found {len(partitions)}")
        boot_partition = None
        root_partition = partitions[0]
        extra_partitions = partitions[1:]
    partition = root_partition
    if partition.partition_type != LINUX_PARTITION_TYPE:
        raise ImageError(f"root partition type is 0x{partition.partition_type:02x}, expected 0x83")
    if partition.start_sector % DEFAULT_START_SECTOR != 0:
        raise ImageError("root partition is not 1 MiB aligned")
    with tempfile.TemporaryDirectory(prefix="wateros-root-verify-") as temporary:
        extracted = Path(temporary) / "root.ext4"
        copy_partition(image, partition, extracted)
        run(["e2fsck", "-fn", str(extracted)])
        superblock = run(["dumpe2fs", "-h", str(extracted)]).stdout
        if f"Block size:               {DEFAULT_BLOCK_SIZE}" not in superblock:
            raise ImageError("root ext4 block size is not 4096")
        feature_line = next(
            (line for line in superblock.splitlines() if line.startswith("Filesystem features:")),
            "",
        )
        features = set(feature_line.partition(":")[2].split())
        if "64bit" not in features:
            raise ImageError("root ext4 must retain the 64bit descriptor layout")
        if "has_journal" in features:
            raise ImageError("root ext4 unexpectedly contains a journal")
        for raw_path in required_paths:
            guest = checked_guest_path(raw_path)
            output = debugfs_command(extracted, f"stat {guest}")
            if "File not found" in output:
                raise ImageError(f"required root path is missing: {guest}")
        for index, (raw_path, expected) in enumerate((expected_files or {}).items()):
            guest = checked_guest_path(raw_path)
            dumped = Path(temporary) / f"manifest-file-{index}"
            output = debugfs_command(extracted, f"dump {guest} {dumped}")
            if "File not found" in output or not dumped.is_file():
                raise ImageError(f"cannot extract required root file: {guest}")
            if dumped.read_bytes() != expected:
                raise ImageError(f"root file content differs from manifest: {guest}")
    if boot_partition is not None:
        verify_boot_partition(image, boot_partition, boot_dir)
    for source, extra_partition in zip(extra_images, extra_partitions):
        _validate_raw_filesystem_image(source)
        with tempfile.TemporaryDirectory(prefix="wateros-extra-verify-") as temporary:
            extracted = Path(temporary) / source.name
            copy_partition(image, extra_partition, extracted)
            with extracted.open("rb") as input_file:
                actual = input_file.read(source.stat().st_size)
            if actual != source.read_bytes():
                raise ImageError(f"filesystem image content differs: {source}")
    return partition


def manifest_paths(path: Path) -> list[str]:
    manifest = load_manifest(path)
    entries = list(manifest.get("directories", [])) + list(manifest.get("files", []))
    paths = []
    for entry in entries:
        if not isinstance(entry, dict):
            raise ImageError("manifest entry must be an object")
        paths.append(str(checked_guest_path(entry.get("path"))))
    return paths


def manifest_file_contents(path: Path, source_root: Path | None = None) -> dict[str, bytes]:
    manifest = load_manifest(path)
    contents: dict[str, bytes] = {}
    for entry in manifest.get("files", []):
        if not isinstance(entry, dict):
            raise ImageError("file entry must be an object")
        guest = str(checked_guest_path(entry.get("path")))
        has_content = "content" in entry
        has_source = "source" in entry
        if has_content == has_source:
            raise ImageError(f"file {guest} must specify exactly one of content/source")
        if has_content:
            content = entry["content"]
            if not isinstance(content, str):
                raise ImageError(f"inline content for {guest} must be a string")
            contents[guest] = content.encode("utf-8")
        else:
            source_value = entry["source"]
            if not isinstance(source_value, str):
                raise ImageError(f"source for {guest} must be a string")
            source = resolve_manifest_source(path, source_value, source_root)
            try:
                contents[guest] = source.read_bytes()
            except OSError as error:
                raise ImageError(f"cannot read manifest source {source}: {error}") from error
    return contents


def parser() -> argparse.ArgumentParser:
    default_manifest = Path(__file__).with_name("rootfs-manifest.json")
    result = argparse.ArgumentParser(description=__doc__)
    subcommands = result.add_subparsers(dest="command", required=True)
    build = subcommands.add_parser("build", help="build a raw MBR/GPT ext4 root image")
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--manifest", type=Path, default=default_manifest)
    build.add_argument("--size-mib", type=int, default=DEFAULT_IMAGE_MIB)
    build.add_argument("--start-sector", type=int, default=DEFAULT_START_SECTOR)
    build.add_argument("--partition-table", choices=("mbr", "gpt"), default="mbr")
    build.add_argument("--uuid", default=DEFAULT_UUID)
    build.add_argument("--label", default=DEFAULT_LABEL)
    build.add_argument("--source-root", type=Path,
                       help="root directory for relative manifest source files")
    build.add_argument("--copy-tree", type=Path,
                       help="copy an existing staging tree as the rootfs (instead of a manifest)")
    build.add_argument("--boot-dir", type=Path,
                       help="VisionFive 2 layout: P1/P2 placeholders, P3 FAT boot (this dir), P4 ext4 rootfs")
    build.add_argument("--boot-size-mib", type=int, default=DEFAULT_BOOT_SIZE_MIB)
    build.add_argument("--boot-layout", choices=("vf2", "boot-root"), default="vf2")
    build.add_argument("--root-size-mib", type=int,
                       help="root partition size when --extra-image is used")
    build.add_argument("--extra-image", dest="extra_images", action="append", type=Path,
                       default=[], help="raw unpartitioned filesystem image for P2+ (or P5+ with --boot-dir)")
    build.add_argument("--extra-partition-type", dest="extra_partition_types", action="append",
                       default=[], help="partition type for each --extra-image")
    build.add_argument("--force", action="store_true")
    verify = subcommands.add_parser("verify", help="verify partition, ext4 and manifest paths")
    verify.add_argument("--image", type=Path, required=True)
    verify.add_argument("--manifest", type=Path, default=default_manifest)
    verify.add_argument("--source-root", type=Path,
                        help="root directory for relative manifest source files")
    verify.add_argument("--copy-tree", type=Path,
                        help="expected paths come from this staging tree instead of a manifest")
    verify.add_argument("--boot-dir", type=Path,
                        help="expected boot files come from this directory")
    verify.add_argument("--boot-layout", choices=("vf2", "boot-root"), default="vf2")
    verify.add_argument("--extra-image", dest="extra_images", action="append", type=Path,
                        default=[], help="expected raw filesystem image for P2+")
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "build":
            required = build_image(args)
            expected = None
            if args.copy_tree is None:
                manifest = args.manifest.resolve()
                expected = manifest_file_contents(manifest,
                                                  args.source_root.resolve()
                                                  if args.source_root else None)
            partition = verify_image(args.output.resolve(), required, expected,
                                     getattr(args, "boot_dir", None),
                                     [path.resolve() for path in args.extra_images],
                                     getattr(args, "boot_layout", "vf2"))
            print(
                f"built {args.output}: start={partition.start_sector} "
                f"sectors={partition.sectors} bytes={args.output.stat().st_size}"
            )
        else:
            expected = None
            if args.copy_tree is not None:
                required = tree_guest_paths(args.copy_tree.resolve())
            else:
                manifest = args.manifest.resolve()
                required = manifest_paths(manifest)
                expected = manifest_file_contents(manifest,
                                                  args.source_root.resolve()
                                                  if args.source_root else None)
            partition = verify_image(args.image.resolve(), required, expected,
                                     getattr(args, "boot_dir", None),
                                     [path.resolve() for path in args.extra_images],
                                     getattr(args, "boot_layout", "vf2"))
            print(
                f"verified {args.image}: start={partition.start_sector} "
                f"sectors={partition.sectors}"
            )
    except ImageError as error:
        print(f"root-image: error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
