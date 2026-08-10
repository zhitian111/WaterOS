#!/usr/bin/env python3
"""Build and verify a small WaterOS physical root disk image without root access."""

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
import zlib
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
MBR_SIGNATURE = b"\x55\xaa"
GPT_SIGNATURE = b"EFI PART"
LINUX_PARTITION_TYPE = 0x83
GPT_LINUX_TYPE_GUID = bytes.fromhex("af3dc60f838472478e793d69d8477de4")


class ImageError(RuntimeError):
    """A malformed manifest/image or failed host tool invocation."""


@dataclass(frozen=True)
class Partition:
    number: int
    partition_type: int
    start_sector: int
    sectors: int

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
    if re.fullmatch(r"/[A-Za-z0-9._+/-]+", raw) is None:
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


def populate_staging(manifest_path: Path, staging: Path) -> list[str]:
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
            source = Path(source_value)
            if not source.is_absolute():
                source = manifest_path.parent / source
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


def _crc32(data: bytes) -> int:
    return zlib.crc32(data) & 0xFFFFFFFF


def parse_gpt_sectors(data: bytes, image_bytes: int) -> list[Partition]:
    """Parse and CRC-check the primary GPT header and entry array."""
    if len(data) < 34 * SECTOR_SIZE or image_bytes % SECTOR_SIZE:
        raise ImageError("short GPT metadata")
    header = bytearray(data[SECTOR_SIZE : 2 * SECTOR_SIZE])
    if header[:8] != GPT_SIGNATURE:
        raise ImageError("missing GPT signature")
    header_size = struct.unpack_from("<I", header, 12)[0]
    if not 92 <= header_size <= SECTOR_SIZE:
        raise ImageError("invalid GPT header size")
    stored_header_crc = struct.unpack_from("<I", header, 16)[0]
    header[16:20] = b"\0" * 4
    if _crc32(header[:header_size]) != stored_header_crc:
        raise ImageError("invalid GPT header CRC")
    current_lba, backup_lba = struct.unpack_from("<QQ", header, 24)
    if current_lba != 1 or backup_lba >= image_bytes // SECTOR_SIZE:
        raise ImageError("invalid GPT header LBA")
    first_usable, last_usable = struct.unpack_from("<QQ", header, 40)
    entries_lba = struct.unpack_from("<Q", header, 72)[0]
    entry_count, entry_size, entries_crc = struct.unpack_from("<III", header, 80)
    if not 1 <= entry_count <= 255 or not 128 <= entry_size <= 4096 or entry_size % 8:
        raise ImageError("invalid GPT entry dimensions")
    entries_bytes = entry_count * entry_size
    entries_end = entries_lba * SECTOR_SIZE + entries_bytes
    if entries_lba == 0 or entries_end > len(data):
        raise ImageError("GPT entry array is outside metadata")
    entries = data[entries_lba * SECTOR_SIZE : entries_end]
    if _crc32(entries) != entries_crc:
        raise ImageError("invalid GPT entry-array CRC")
    total_sectors = image_bytes // SECTOR_SIZE
    if first_usable == 0 or first_usable > last_usable or last_usable >= total_sectors:
        raise ImageError("invalid GPT usable range")
    partitions: list[Partition] = []
    for index in range(entry_count):
        entry = entries[index * entry_size : (index + 1) * entry_size]
        if not any(entry[:16]):
            continue
        start, end = struct.unpack_from("<QQ", entry, 32)
        if start < first_usable or end < start or end > last_usable:
            raise ImageError(f"invalid GPT partition entry {index + 1}")
        partition = Partition(index + 1, LINUX_PARTITION_TYPE, start, end - start + 1)
        if partition.byte_offset + partition.byte_length > image_bytes:
            raise ImageError(f"partition {index + 1} extends beyond image")
        partitions.append(partition)
    for index, left in enumerate(partitions):
        left_end = left.start_sector + left.sectors
        for right in partitions[index + 1 :]:
            right_end = right.start_sector + right.sectors
            if left.start_sector < right_end and right.start_sector < left_end:
                raise ImageError("overlapping GPT partitions")
    return partitions


def read_partitions(image: Path) -> list[Partition]:
    try:
        image_bytes = image.stat().st_size
        with image.open("rb") as source:
            sector = source.read(SECTOR_SIZE)
            metadata = sector + source.read(33 * SECTOR_SIZE)
    except OSError as error:
        raise ImageError(f"cannot read image {image}: {error}") from error
    partitions = parse_mbr_sector(sector, image_bytes)
    if partitions and partitions[0].partition_type == 0xEE:
        return parse_gpt_sectors(metadata, image_bytes)
    return partitions


def verify_gpt_backup(image: Path) -> None:
    """Verify the GPT backup header and entry array without loading the image."""
    image_bytes = image.stat().st_size
    total_sectors = image_bytes // SECTOR_SIZE
    with image.open("rb") as source:
        source.seek(SECTOR_SIZE)
        primary = source.read(SECTOR_SIZE)
        if primary[:8] != GPT_SIGNATURE:
            raise ImageError("missing GPT primary header")
        backup_lba = struct.unpack_from("<Q", primary, 32)[0]
        entries_lba = struct.unpack_from("<Q", primary, 72)[0]
        entry_count, entry_size = struct.unpack_from("<II", primary, 80)
        entries_bytes = entry_count * entry_size
        entry_sectors = (entries_bytes + SECTOR_SIZE - 1) // SECTOR_SIZE
        if backup_lba != total_sectors - 1 or backup_lba <= entry_sectors:
            raise ImageError("invalid GPT backup header LBA")
        source.seek(entries_lba * SECTOR_SIZE)
        primary_entries = source.read(entries_bytes)
        source.seek((backup_lba - entry_sectors) * SECTOR_SIZE)
        backup_entries = source.read(entries_bytes)
        source.seek(backup_lba * SECTOR_SIZE)
        backup = bytearray(source.read(SECTOR_SIZE))
    if len(backup) != SECTOR_SIZE or backup[:8] != GPT_SIGNATURE:
        raise ImageError("missing GPT backup header")
    header_size = struct.unpack_from("<I", backup, 12)[0]
    stored_crc = struct.unpack_from("<I", backup, 16)[0]
    backup[16:20] = b"\0" * 4
    if not 92 <= header_size <= SECTOR_SIZE or _crc32(backup[:header_size]) != stored_crc:
        raise ImageError("invalid GPT backup header CRC")
    current_lba, declared_backup = struct.unpack_from("<QQ", backup, 24)
    if current_lba != backup_lba or declared_backup != 1:
        raise ImageError("GPT backup header LBA pair is inconsistent")
    declared_entries_lba = struct.unpack_from("<Q", backup, 72)[0]
    if declared_entries_lba != backup_lba - entry_sectors:
        raise ImageError("GPT backup entry-array LBA is inconsistent")
    if _crc32(backup_entries) != struct.unpack_from("<I", backup, 88)[0]:
        raise ImageError("invalid GPT backup entry-array CRC")
    if primary_entries != backup_entries:
        raise ImageError("GPT primary and backup entry arrays differ")


def make_partition_table(
    image: Path,
    image_bytes: int,
    start_sector: int,
    table_type: str = "mbr",
    data_size_mib: int = 0,
) -> Partition:
    total_sectors = image_bytes // SECTOR_SIZE
    data_sectors = _data_partition_sectors(data_size_mib)
    root_sectors = total_sectors - start_sector - data_sectors
    if root_sectors <= 0 or root_sectors % 8:
        raise ImageError("image is too small for requested partition start")
    data_start = start_sector + root_sectors
    if table_type == "mbr":
        specification = (
            "label: dos\n"
            f"label-id: {DEFAULT_DISK_ID}\n"
            "unit: sectors\n\n"
            f"{start_sector},{root_sectors},83\n"
        )
        if data_sectors:
            specification += f"{data_start},{data_sectors},83\n"
        run(["sfdisk", "--quiet", str(image)], input_text=specification)
    elif table_type == "gpt":
        return make_gpt_partition_table(image, image_bytes, start_sector, data_size_mib)
    else:
        raise ImageError(f"unsupported partition table: {table_type}")
    partition = read_partitions(image)
    if len(partition) != (2 if data_sectors else 1):
        raise ImageError("partitioning tool created an unexpected partition count")
    return partition[0]


def _data_partition_sectors(data_size_mib: int) -> int:
    if data_size_mib < 0:
        raise ImageError("data partition size must not be negative")
    if data_size_mib == 0:
        return 0
    sectors = data_size_mib * 1024 * 1024 // SECTOR_SIZE
    if sectors < 8 or sectors % 8:
        raise ImageError("data partition size must be at least 4 KiB and 8-sector aligned")
    return sectors


def build_image(args: argparse.Namespace) -> list[str]:
    image = args.output.resolve()
    if image.exists() and not args.force:
        raise ImageError(f"output already exists (use --force): {image}")
    image_bytes = args.size_mib * 1024 * 1024
    if image_bytes % SECTOR_SIZE != 0 or args.size_mib < 16:
        raise ImageError("image size must be at least 16 MiB and sector aligned")
    image.parent.mkdir(parents=True, exist_ok=True)
    temporary_image: Path | None = None
    try:
        with tempfile.TemporaryDirectory(prefix="wateros-root-staging-") as temporary, \
             tempfile.TemporaryDirectory(prefix="wateros-data-staging-") as data_temporary:
            staging = Path(temporary)
            data_staging = Path(data_temporary)
            required_paths = populate_staging(args.manifest.resolve(), staging)
            data_manifest_value = getattr(args, "data_manifest", None)
            data_size_mib = getattr(args, "data_size_mib", 0)
            if data_manifest_value is None and data_size_mib:
                raise ImageError("data partition requires --data-manifest")
            data_required_paths: list[str] = []
            if data_manifest_value is not None:
                if not data_size_mib:
                    raise ImageError("--data-manifest requires --data-size-mib")
                data_required_paths = populate_staging(
                    Path(data_manifest_value).resolve(), data_staging
                )
            descriptor, raw_path = tempfile.mkstemp(
                prefix=f".{image.name}.", suffix=".tmp", dir=image.parent
            )
            os.close(descriptor)
            temporary_image = Path(raw_path)
            with temporary_image.open("wb") as output:
                output.truncate(image_bytes)
            partition = make_partition_table(
                temporary_image,
                image_bytes,
                args.start_sector,
                getattr(args, "partition_table", "mbr"),
                data_size_mib,
            )
            partitions = read_partitions(temporary_image)
            if not partitions or partitions[0] != partition:
                raise ImageError("root partition table result is inconsistent")
            data_partition = partitions[1] if data_size_mib else None
            if partition.partition_type != LINUX_PARTITION_TYPE:
                raise ImageError("root partition has unexpected MBR type")
            if partition.byte_length % DEFAULT_BLOCK_SIZE != 0:
                raise ImageError("partition size is not aligned to ext4 block size")
            blocks = partition.byte_length // DEFAULT_BLOCK_SIZE
            environment = os.environ.copy()
            environment.setdefault("E2FSPROGS_FAKE_TIME", "1704067200")
            _make_ext4(
                temporary_image,
                partition,
                staging,
                args.label,
                args.uuid,
                environment,
            )
            if data_partition is not None:
                _make_ext4(
                    temporary_image,
                    data_partition,
                    data_staging,
                    getattr(args, "data_label", "WATEROS_DATA"),
                    getattr(args, "data_uuid", "574f5300-0000-4000-8000-000000000002"),
                    environment,
                )
            # Do not replace a known-good image before the replacement passes
            # both filesystem and manifest validation.
            verify_image(
                temporary_image,
                required_paths,
                manifest_file_contents(args.manifest.resolve()),
                data_required_paths,
                manifest_file_contents(Path(data_manifest_value).resolve())
                if data_manifest_value is not None
                else None,
            )
            os.replace(temporary_image, image)
            temporary_image = None
            return required_paths
    finally:
        if temporary_image is not None:
            temporary_image.unlink(missing_ok=True)


def _make_ext4(
    image: Path,
    partition: Partition,
    staging: Path,
    label: str,
    uuid: str,
    environment: dict[str, str],
) -> None:
    if partition.byte_length % DEFAULT_BLOCK_SIZE:
        raise ImageError("partition size is not aligned to ext4 block size")
    blocks = partition.byte_length // DEFAULT_BLOCK_SIZE
    command = [
        "mkfs.ext4", "-q", "-F", "-b", str(DEFAULT_BLOCK_SIZE),
        "-L", label, "-U", uuid,
        # another_ext4 currently requires the 64-bit group descriptor layout.
        "-O", "^has_journal",
        "-E", f"offset={partition.byte_offset},lazy_itable_init=0,lazy_journal_init=0",
        "-d", str(staging), str(image), str(blocks),
    ]
    try:
        subprocess.run(command, env=environment, check=True)
    except FileNotFoundError as error:
        raise ImageError("required host tool not found: mkfs.ext4") from error
    except subprocess.CalledProcessError as error:
        raise ImageError(f"mkfs.ext4 failed with status {error.returncode}") from error


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
    image: Path,
    required_paths: Iterable[str],
    expected_files: dict[str, bytes] | None = None,
    data_required_paths: Iterable[str] | None = None,
    data_expected_files: dict[str, bytes] | None = None,
) -> Partition:
    partitions = read_partitions(image)
    with image.open("rb") as source:
        protective_type = source.read(SECTOR_SIZE)[446 + 4]
    if protective_type == 0xEE:
        verify_gpt_backup(image)
    wants_data = data_required_paths is not None or data_expected_files is not None
    expected_count = 2 if wants_data else 1
    if len(partitions) != expected_count:
        raise ImageError(f"expected {expected_count} partition(s), found {len(partitions)}")
    partition = partitions[0]
    if partition.partition_type != LINUX_PARTITION_TYPE:
        raise ImageError(f"root partition type is 0x{partition.partition_type:02x}, expected 0x83")
    if partition.start_sector % DEFAULT_START_SECTOR != 0:
        raise ImageError("root partition is not 1 MiB aligned")
    with tempfile.TemporaryDirectory(prefix="wateros-root-verify-") as temporary:
        _verify_ext4_partition(
            image, partition, Path(temporary) / "root.ext4", required_paths,
            expected_files, "root",
        )
        if wants_data:
            data_partition = partitions[1]
            if data_partition.partition_type != LINUX_PARTITION_TYPE:
                raise ImageError("data partition is not Linux type 0x83")
            _verify_ext4_partition(
                image, data_partition, Path(temporary) / "data.ext4",
                data_required_paths or (), data_expected_files, "data",
            )
    return partition


def _verify_ext4_partition(
    image: Path,
    partition: Partition,
    extracted: Path,
    required_paths: Iterable[str],
    expected_files: dict[str, bytes] | None,
    name: str,
) -> None:
    copy_partition(image, partition, extracted)
    run(["e2fsck", "-fn", str(extracted)])
    superblock = run(["dumpe2fs", "-h", str(extracted)]).stdout
    if f"Block size:               {DEFAULT_BLOCK_SIZE}" not in superblock:
        raise ImageError(f"{name} ext4 block size is not 4096")
    feature_line = next(
        (line for line in superblock.splitlines() if line.startswith("Filesystem features:")),
        "",
    )
    features = set(feature_line.partition(":")[2].split())
    if "64bit" not in features:
        raise ImageError(f"{name} ext4 must retain the 64bit descriptor layout")
    if "has_journal" in features:
        raise ImageError(f"{name} ext4 unexpectedly contains a journal")
    for raw_path in required_paths:
        guest = checked_guest_path(raw_path)
        output = debugfs_command(extracted, f"stat {guest}")
        if "File not found" in output:
            raise ImageError(f"required {name} path is missing: {guest}")
    for index, (raw_path, expected) in enumerate((expected_files or {}).items()):
        guest = checked_guest_path(raw_path)
        dumped = extracted.parent / f"{name}-manifest-file-{index}"
        output = debugfs_command(extracted, f"dump {guest} {dumped}")
        if "File not found" in output or not dumped.is_file():
            raise ImageError(f"cannot extract required {name} file: {guest}")
        if dumped.read_bytes() != expected:
            raise ImageError(f"{name} file content differs from manifest: {guest}")


def make_gpt_partition_table(
    image: Path, image_bytes: int, start_sector: int, data_size_mib: int = 0
) -> Partition:
    total_sectors = image_bytes // SECTOR_SIZE
    entry_count, entry_size = 128, 128
    entry_sectors = entry_count * entry_size // SECTOR_SIZE
    first_usable = 2 + entry_sectors
    last_usable = total_sectors - entry_sectors - 2
    if start_sector < first_usable or last_usable <= start_sector:
        raise ImageError("image is too small for GPT metadata and root partition")
    data_sectors = _data_partition_sectors(data_size_mib)
    partition_sectors = ((last_usable - start_sector + 1 - data_sectors) // 8) * 8
    if partition_sectors <= 0:
        raise ImageError("GPT root partition is too small")
    partition_end = start_sector + partition_sectors - 1
    data_start = partition_end + 1
    data_end = data_start + data_sectors - 1
    if data_sectors and data_end > last_usable:
        raise ImageError("GPT data partition is outside usable range")
    entries = bytearray(entry_count * entry_size)
    unique_root = bytes.fromhex("01000000000040008000000000000001")
    entries[:16] = GPT_LINUX_TYPE_GUID
    entries[16:32] = unique_root
    struct.pack_into("<QQ", entries, 32, start_sector, partition_end)
    name = "WaterOS root".encode("utf-16le")
    entries[56 : 56 + len(name)] = name
    if data_sectors:
        offset = entry_size
        entries[offset : offset + 16] = GPT_LINUX_TYPE_GUID
        entries[offset + 16 : offset + 32] = bytes.fromhex(
            "02000000000040008000000000000001"
        )
        struct.pack_into("<QQ", entries, offset + 32, data_start, data_end)
        data_name = "WaterOS data".encode("utf-16le")
        entries[offset + 56 : offset + 56 + len(data_name)] = data_name
    entries_crc = _crc32(entries)
    header = bytearray(SECTOR_SIZE)
    header[:8] = GPT_SIGNATURE
    struct.pack_into("<II", header, 8, 0x00010000, 92)
    struct.pack_into("<QQQQ", header, 24, 1, total_sectors - 1, first_usable, last_usable)
    struct.pack_into("<QIII", header, 72, 2, entry_count, entry_size, entries_crc)
    header[16:20] = b"\0" * 4
    struct.pack_into("<I", header, 16, _crc32(header[:92]))
    backup_header = bytearray(header)
    struct.pack_into("<QQ", backup_header, 24, total_sectors - 1, 1)
    struct.pack_into("<Q", backup_header, 72, total_sectors - entry_sectors - 1)
    struct.pack_into("<I", backup_header, 16, 0)
    struct.pack_into("<I", backup_header, 16, _crc32(backup_header[:92]))
    with image.open("r+b") as target:
        protective = bytearray(SECTOR_SIZE)
        struct.pack_into("<I", protective, 440, int(DEFAULT_DISK_ID, 16))
        protective[446 + 4] = 0xEE
        struct.pack_into("<II", protective, 446 + 8, 1, min(total_sectors - 1, 0xFFFFFFFF))
        protective[510:512] = MBR_SIGNATURE
        target.seek(0)
        target.write(protective)
        target.seek(2 * SECTOR_SIZE)
        target.write(entries)
        target.seek((total_sectors - entry_sectors - 1) * SECTOR_SIZE)
        target.write(entries)
        target.seek(SECTOR_SIZE)
        target.write(header)
        target.seek((total_sectors - 1) * SECTOR_SIZE)
        target.write(backup_header)
    return Partition(1, LINUX_PARTITION_TYPE, start_sector, partition_sectors)


def manifest_paths(path: Path) -> list[str]:
    manifest = load_manifest(path)
    entries = list(manifest.get("directories", [])) + list(manifest.get("files", []))
    paths = []
    for entry in entries:
        if not isinstance(entry, dict):
            raise ImageError("manifest entry must be an object")
        paths.append(str(checked_guest_path(entry.get("path"))))
    return paths


def manifest_file_contents(path: Path) -> dict[str, bytes]:
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
            source = Path(source_value)
            if not source.is_absolute():
                source = path.parent / source
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
    build.add_argument("--data-manifest", type=Path)
    build.add_argument("--data-size-mib", type=int, default=0)
    build.add_argument("--data-uuid", default="574f5300-0000-4000-8000-000000000002")
    build.add_argument("--data-label", default="WATEROS_DATA")
    build.add_argument("--uuid", default=DEFAULT_UUID)
    build.add_argument("--label", default=DEFAULT_LABEL)
    build.add_argument("--force", action="store_true")
    verify = subcommands.add_parser("verify", help="verify partition, ext4 and manifest paths")
    verify.add_argument("--image", type=Path, required=True)
    verify.add_argument("--manifest", type=Path, default=default_manifest)
    verify.add_argument("--data-manifest", type=Path)
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "build":
            required = build_image(args)
            manifest = args.manifest.resolve()
            partition = verify_image(
                args.output.resolve(),
                required,
                manifest_file_contents(manifest),
                manifest_paths(args.data_manifest.resolve()) if args.data_manifest else None,
                manifest_file_contents(args.data_manifest.resolve()) if args.data_manifest else None,
            )
            print(
                f"built {args.output}: start={partition.start_sector} "
                f"sectors={partition.sectors} bytes={args.output.stat().st_size}"
            )
        else:
            manifest = args.manifest.resolve()
            partition = verify_image(
                args.image.resolve(),
                manifest_paths(manifest),
                manifest_file_contents(manifest),
                manifest_paths(args.data_manifest.resolve()) if args.data_manifest else None,
                manifest_file_contents(args.data_manifest.resolve()) if args.data_manifest else None,
            )
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
