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


def crc32(data: bytes) -> int:
    value = 0xFFFFFFFF
    for byte in data:
        value ^= byte
        for _ in range(8):
            value = (value >> 1) ^ 0xEDB88320 if value & 1 else value >> 1
    return (~value) & 0xFFFFFFFF


def parse_gpt_image(image: Path) -> list[Partition]:
    image_bytes = image.stat().st_size
    total_sectors = image_bytes // SECTOR_SIZE
    with image.open("rb") as source:
        source.seek(SECTOR_SIZE)
        header = source.read(SECTOR_SIZE)
        if header[:8] != GPT_SIGNATURE or len(header) != SECTOR_SIZE:
            raise ImageError("missing GPT header")
        header_size = struct.unpack_from("<I", header, 12)[0]
        current_lba, backup_lba = struct.unpack_from("<QQ", header, 24)
        first_usable, last_usable = struct.unpack_from("<QQ", header, 40)
        entries_lba, entry_count, entry_size, entries_crc = struct.unpack_from(
            "<QIII", header, 72
        )
        if not 92 <= header_size <= SECTOR_SIZE or current_lba != 1:
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
        with tempfile.TemporaryDirectory(prefix="wateros-root-staging-") as temporary:
            staging = Path(temporary)
            required_paths = populate_staging(args.manifest.resolve(), staging)
            descriptor, raw_path = tempfile.mkstemp(
                prefix=f".{image.name}.", suffix=".tmp", dir=image.parent
            )
            os.close(descriptor)
            temporary_image = Path(raw_path)
            with temporary_image.open("wb") as output:
                output.truncate(image_bytes)
            partition = make_partition_table(temporary_image, image_bytes, args.start_sector,
                                             getattr(args, "partition_table", "mbr"))
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
            # Do not replace a known-good image before the replacement passes
            # both filesystem and manifest validation.
            verify_image(
                temporary_image,
                required_paths,
                manifest_file_contents(args.manifest.resolve()),
            )
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
    image: Path, required_paths: Iterable[str], expected_files: dict[str, bytes] | None = None
) -> Partition:
    partitions = read_partitions(image)
    if len(partitions) != 1:
        raise ImageError(f"expected one root partition, found {len(partitions)}")
    partition = partitions[0]
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
    build.add_argument("--uuid", default=DEFAULT_UUID)
    build.add_argument("--label", default=DEFAULT_LABEL)
    build.add_argument("--force", action="store_true")
    verify = subcommands.add_parser("verify", help="verify partition, ext4 and manifest paths")
    verify.add_argument("--image", type=Path, required=True)
    verify.add_argument("--manifest", type=Path, default=default_manifest)
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "build":
            required = build_image(args)
            manifest = args.manifest.resolve()
            partition = verify_image(
                args.output.resolve(), required, manifest_file_contents(manifest)
            )
            print(
                f"built {args.output}: start={partition.start_sector} "
                f"sectors={partition.sectors} bytes={args.output.stat().st_size}"
            )
        else:
            manifest = args.manifest.resolve()
            partition = verify_image(
                args.image.resolve(), manifest_paths(manifest), manifest_file_contents(manifest)
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
