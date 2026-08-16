#!/usr/bin/env python3
"""Prepare immutable BuildStorm raw-image masters from organizer inputs."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path


OS_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_INPUT_ROOT = Path("/home/zhitian/Downloads")
DEFAULT_OUTPUT_ROOT = OS_ROOT / "tem/perf/buildstorm-singlecore/images"
GUEST_SCRIPT_PATH = "/glibc/buildstorm_testcode.sh"
RAW_IMAGE_SIZE = 15_032_385_536
SPARSE_BLOCK_SIZE = 64 * 1024
SCRIPT_NAME = "buildstorm_testcode.recovered.sh"
SCRIPT_SHA256 = "84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb"
ARCH_CONFIG = {
    "rv": {
        "image": "sdcard-rv-pub.img.gz",
        "sha256": "cba87f43ae569bcf2b8e4614f75cec1bf51bedb2804626fe466fcce3861df6f1",
    },
    "la": {
        "image": "sdcard-la-pub.img.gz",
        "sha256": "2c411447274fbd83505d2fac505a5d9e8ed8ff3bdfc3d2d6cbdb8f61ff7d90d2",
    },
}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def output_image_path(output_root: Path, arch: str) -> Path:
    return output_root / f"sdcard-{arch}-pub-prepared.img"


def manifest_path(image: Path) -> Path:
    return image.with_suffix(image.suffix + ".json")


def debugfs_commands(source_script: Path) -> tuple[str, ...]:
    return (
        f"rm {GUEST_SCRIPT_PATH}",
        f"write {source_script} {GUEST_SCRIPT_PATH}",
        f"set_inode_field {GUEST_SCRIPT_PATH} mode 0100755",
    )


def run_checked(argv: list[str], *, capture: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        argv,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.STDOUT if capture else None,
        check=True,
    )


def validate_hash(path: Path, expected: str, label: str) -> str:
    if not path.is_file():
        raise RuntimeError(f"{label} does not exist: {path}")
    observed = sha256_file(path)
    if observed != expected:
        raise RuntimeError(f"{label} sha256 mismatch: expected {expected}, observed {observed}")
    return observed


def decompress_sparse(source: Path, destination: Path) -> int:
    """Decompress while seeking over all-zero chunks to avoid allocating image holes."""
    total = 0
    with gzip.open(source, "rb") as compressed, destination.open("xb") as raw:
        while chunk := compressed.read(4 * 1024 * 1024):
            total += len(chunk)
            for offset in range(0, len(chunk), SPARSE_BLOCK_SIZE):
                block = chunk[offset : offset + SPARSE_BLOCK_SIZE]
                if not block.strip(b"\0"):
                    raw.seek(len(block), os.SEEK_CUR)
                else:
                    raw.write(block)
        raw.truncate(total)
    return total


def existing_master_is_reusable(image: Path, source_hash: str) -> bool:
    metadata_path = manifest_path(image)
    if not image.is_file() or image.stat().st_size != RAW_IMAGE_SIZE or not metadata_path.is_file():
        return False
    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return False
    return (
        metadata.get("source_compressed_sha256") == source_hash
        and metadata.get("guest_script_sha256") == SCRIPT_SHA256
        and metadata.get("guest_script_path") == GUEST_SCRIPT_PATH
        and metadata.get("raw_size") == RAW_IMAGE_SIZE
        and metadata.get("prepared_raw_sha256") == sha256_file(image)
        and metadata.get("e2fsck_exit_code") == 0
    )


def prepare(args: argparse.Namespace) -> dict[str, object]:
    config = ARCH_CONFIG[args.arch]
    input_root = args.input_root.resolve()
    output_root = args.output_root.resolve()
    source = input_root / str(config["image"])
    script = args.script.resolve()
    output = output_image_path(output_root, args.arch)
    source_hash = validate_hash(source, str(config["sha256"]), "compressed image")
    script_hash = validate_hash(script, SCRIPT_SHA256, "BuildStorm script")
    plan: dict[str, object] = {
        "arch": args.arch,
        "source": str(source),
        "source_compressed_sha256": source_hash,
        "guest_script_source": str(script),
        "guest_script_path": GUEST_SCRIPT_PATH,
        "guest_script_sha256": script_hash,
        "output": str(output),
        "raw_size": RAW_IMAGE_SIZE,
        "debugfs_commands": list(debugfs_commands(script)),
    }
    if args.dry_run:
        plan["status"] = "dry_run"
        return plan
    if output.exists() and not args.force and existing_master_is_reusable(output, source_hash):
        plan["status"] = "reused"
        return plan
    if output.exists() and not args.force:
        raise RuntimeError(f"output already exists and is not reusable; pass --force: {output}")

    output_root.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.tmp-{os.getpid()}")
    dumped = output.with_name(f".{output.name}.script-{os.getpid()}")
    try:
        raw_size = decompress_sparse(source, temporary)
        if raw_size != RAW_IMAGE_SIZE or temporary.stat().st_size != RAW_IMAGE_SIZE:
            raise RuntimeError(
                f"unexpected raw image size: expected {RAW_IMAGE_SIZE}, observed {raw_size}"
            )
        debugfs_log: list[str] = []
        for command in debugfs_commands(script):
            completed = run_checked(["debugfs", "-w", "-R", command, str(temporary)])
            debugfs_log.append(completed.stdout)
        stat_result = run_checked(["debugfs", "-R", f"stat {GUEST_SCRIPT_PATH}", str(temporary)])
        run_checked(["debugfs", "-R", f"dump {GUEST_SCRIPT_PATH} {dumped}", str(temporary)])
        dumped_hash = sha256_file(dumped)
        if dumped_hash != SCRIPT_SHA256:
            raise RuntimeError(
                f"guest script dump sha256 mismatch: expected {SCRIPT_SHA256}, observed {dumped_hash}"
            )
        e2fsck = subprocess.run(
            ["e2fsck", "-fn", str(temporary)],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        if e2fsck.returncode != 0:
            raise RuntimeError(f"e2fsck -fn failed with {e2fsck.returncode}:\n{e2fsck.stdout}")
        raw_hash = sha256_file(temporary)
        os.replace(temporary, output)
        metadata = {
            **plan,
            "status": "prepared",
            "prepared_raw_sha256": raw_hash,
            "guest_script_dump_sha256": dumped_hash,
            "debugfs_stat": stat_result.stdout,
            "debugfs_log": debugfs_log,
            "e2fsck_exit_code": e2fsck.returncode,
            "e2fsck_output": e2fsck.stdout,
        }
        manifest_path(output).write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        return metadata
    finally:
        temporary.unlink(missing_ok=True)
        dumped.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=sorted(ARCH_CONFIG), required=True)
    parser.add_argument("--input-root", type=Path, default=DEFAULT_INPUT_ROOT)
    parser.add_argument("--output-root", type=Path, default=DEFAULT_OUTPUT_ROOT)
    parser.add_argument("--script", type=Path, default=DEFAULT_INPUT_ROOT / SCRIPT_NAME)
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    try:
        result = prepare(args)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"prepare-buildstorm-image: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
