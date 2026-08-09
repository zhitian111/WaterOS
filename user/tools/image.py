#!/usr/bin/env python3
"""Create, augment and inspect unpartitioned WaterOS EXT4 images."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import uuid
from pathlib import Path
from typing import Iterable

USER_ROOT = Path(__file__).resolve().parents[1]
BUILD_ROOT = USER_ROOT / "build"
SOURCE_DATE_EPOCH = "1704067200"
EXT4_FEATURES = (
    # WaterOS 的 `another_ext4` 后端要求 64 字节块组描述符；即使镜像很小，
    # e2fsprogs 也只有在启用 ext4 `64bit` feature 时才生成这种布局。
    "extent,filetype,sparse_super,64bit,^metadata_csum,^metadata_csum_seed,^dir_index,"
    "^orphan_file,^encrypt,^casefold"
)
OVERLAY_ALLOWED_PREFIXES = (
    "/bin", "/sbin", "/usr/bin", "/usr/sbin", "/etc/wateros",
    "/opt/wateros", "/root", "/var/lib/wateros",
)
# These directories may need to be created solely to reach an allowed subtree
# (for example `/opt` before `/opt/wateros`).  Files at these paths are never
# accepted, so this does not broaden the writable overlay surface.
OVERLAY_STRUCTURAL_PARENTS = ("/usr", "/etc", "/opt", "/var", "/var/lib")
PROTECTED_PREFIXES = ("/glibc", "/musl")


class ImageError(RuntimeError):
    """A deterministic image creation or validation error."""


def default_image_path(arch: str, profile: str) -> Path:
    return BUILD_ROOT / "images" / f"wateros-{arch}-{profile}.ext4"


def default_overlay_path(base: Path, arch: str, profile: str) -> Path:
    del arch  # The base image name already carries the architecture in normal use.
    return BUILD_ROOT / "images" / f"{base.stem}-{profile}.ext4"


def _run(command: list[str], *, env: dict[str, str] | None = None,
         capture: bool = False, accepted: tuple[int, ...] = (0,)) -> subprocess.CompletedProcess[str]:
    print("[image]", " ".join(command), flush=True)
    completed = subprocess.run(command, env=env, text=True,
                               capture_output=capture)
    if completed.returncode not in accepted:
        detail = (completed.stderr or completed.stdout or "").strip()
        raise ImageError(f"command failed ({completed.returncode}): {' '.join(command)}\n{detail}")
    return completed


def _run_debugfs_batch(image: Path, commands: Iterable[str]) -> None:
    command_list = list(commands)
    if not command_list:
        return
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", delete=False,
                                     prefix="wateros-debugfs-", suffix=".cmd") as batch:
        batch.write("\n".join(command_list) + "\n")
        batch_path = Path(batch.name)
    try:
        env = os.environ.copy()
        env["SOURCE_DATE_EPOCH"] = SOURCE_DATE_EPOCH
        env["E2FSPROGS_FAKE_TIME"] = SOURCE_DATE_EPOCH
        completed = _run(["debugfs", "-w", "-f", str(batch_path), str(image)],
                         env=env, capture=True)
        diagnostics = (completed.stdout or "") + (completed.stderr or "")
        # debugfs historically returns zero for a number of individual command
        # failures.  Treat its textual diagnostics as part of the API contract.
        failure_markers = ("couldn't", "file not found", "usage:", "invalid field",
                           "command not found", "error: ")
        lowered = diagnostics.lower()
        if any(marker in lowered for marker in failure_markers):
            raise ImageError(f"debugfs batch reported an error:\n{diagnostics.strip()}")
    finally:
        batch_path.unlink(missing_ok=True)


def _inode_metadata_commands(logical: str) -> list[str]:
    path = _debugfs_quote(logical)
    commands = [f"set_inode_field {path} uid 0", f"set_inode_field {path} gid 0"]
    for field in ("atime", "ctime", "mtime", "crtime"):
        commands.append(f"set_inode_field {path} {field} {SOURCE_DATE_EPOCH}")
    return commands


def _normalize_image_metadata(image: Path, staging: Path,
                              arch: str, profile: str) -> None:
    # mke2fs randomizes the directory hash seed even when UUID and time are
    # fixed.  dir_index is disabled for the compatibility profile, but the
    # superblock field must still be normalized for byte-reproducible images.
    commands: list[str] = [f"set_super_value hash_seed {_fixed_uuid(arch, profile)}"]
    for field in ("mtime", "wtime", "lastcheck", "mkfs_time"):
        commands.append(f"set_super_value {field} {SOURCE_DATE_EPOCH}")
    for logical in ["/", *("/" + entry.relative_to(staging).as_posix()
                           for entry in _iter_entries(staging))]:
        commands.extend(_inode_metadata_commands(logical))
    _run_debugfs_batch(image, commands)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _write_sidecars(image: Path, manifest: list[dict[str, object]]) -> None:
    manifest_path = image.with_suffix(image.suffix + ".manifest.json")
    checksum_path = image.with_suffix(image.suffix + ".sha256")
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                             encoding="utf-8")
    checksum_path.write_text(f"{_sha256(image)}  {image.name}\n", encoding="utf-8")
    print(f"[image] manifest: {manifest_path}")
    print(f"[image] checksum: {checksum_path}")


def _iter_entries(root: Path) -> Iterable[Path]:
    return sorted(root.rglob("*"), key=lambda path: (len(path.relative_to(root).parts),
                                                     path.relative_to(root).as_posix()))


def staging_manifest(root: Path) -> list[dict[str, object]]:
    result: list[dict[str, object]] = []
    for entry in _iter_entries(root):
        relative = "/" + entry.relative_to(root).as_posix()
        stat = entry.lstat()
        item: dict[str, object] = {"path": relative, "mode": stat.st_mode & 0o7777}
        if entry.is_symlink():
            item.update(kind="symlink", target=os.readlink(entry))
        elif entry.is_dir():
            item.update(kind="directory")
        else:
            item.update(kind="file", size=stat.st_size, sha256=_sha256(entry))
        result.append(item)
    return result


def _fixed_uuid(arch: str, profile: str) -> str:
    return str(uuid.uuid5(uuid.NAMESPACE_URL, f"https://wateros.local/rootfs/{arch}/{profile}"))


def _label(arch: str, profile: str) -> str:
    abbreviated = {"minimal": "min", "operator": "operator", "nanox": "nanox"}.get(
        profile, profile
    )
    return f"wos-{arch}-{abbreviated}"[:16]


def _validate_wateros_ext4_format(image: Path, block_size: int,
                                  inode_size: int) -> None:
    """拒绝 Linux 能读取、但 WaterOS 当前后端无法挂载的 EXT4 镜像。"""
    header = _run(["dumpe2fs", "-h", str(image)], capture=True).stdout or ""
    expected = {
        "Block size": str(block_size),
        "Inode size": str(inode_size),
        "Group descriptor size": "64",
    }
    for field, value in expected.items():
        match = re.search(rf"^{re.escape(field)}:\s+(\S+)", header, re.MULTILINE)
        actual = match.group(1) if match else None
        if actual != value:
            raise ImageError(
                f"WaterOS-incompatible EXT4 {field.lower()}: "
                f"expected {value}, got {actual or 'missing'}"
            )
    feature_match = re.search(r"^Filesystem features:\s+(.+)$", header, re.MULTILINE)
    features = set(feature_match.group(1).split()) if feature_match else set()
    if "64bit" not in features:
        raise ImageError("WaterOS EXT4 images require 64bit for 64-byte descriptors")
    unsupported = features.intersection(
        {"metadata_csum", "metadata_csum_seed", "orphan_file", "encrypt", "casefold"}
    )
    if unsupported:
        raise ImageError(
            "WaterOS-incompatible EXT4 features: " + ", ".join(sorted(unsupported))
        )


def _validate_image(image: Path, block_size: int, inode_size: int) -> None:
    _run(["e2fsck", "-fn", str(image)])
    _validate_wateros_ext4_format(image, block_size, inode_size)
    stats: dict[str, str] = {}
    for path in ("/bin/busybox", "/bin/sh", "/etc/wateros-release",
                 "/var/lib/wateros/packages.json", "/root", "/tmp",
                 "/var/tmp", "/dev/shm"):
        stat = _debugfs_stat(image, path)
        if stat is None:
            raise ImageError(f"required image path is missing: {path}")
        stats[path] = stat
    if "Type: regular" not in stats["/bin/busybox"] or "Mode:  0755" not in stats["/bin/busybox"]:
        raise ImageError("/bin/busybox must be a regular executable (0755)")
    if "Type: symlink" not in stats["/bin/sh"] or "busybox" not in stats["/bin/sh"]:
        raise ImageError("/bin/sh must be a symbolic link to BusyBox")
    for path, mode in (("/root", "0700"), ("/tmp", "01777"),
                       ("/var/tmp", "01777"), ("/dev/shm", "01777")):
        if f"Mode:  {mode}" not in stats[path]:
            raise ImageError(f"{path} has the wrong permissions; expected {mode}")
    for path, stat in stats.items():
        if not re.search(r"User:\s+0\s+Group:\s+0", stat):
            raise ImageError(f"{path} is not owned by root:root")


def create_image(staging: Path, output: Path, arch: str, profile: str,
                 image_size_mb: int, block_size: int, inode_size: int) -> Path:
    staging = staging.resolve()
    output = output.resolve()
    if not staging.is_dir():
        raise ImageError(f"staging directory does not exist: {staging}")
    if image_size_mb < 16:
        raise ImageError("image size must be at least 16 MiB")
    if block_size not in (1024, 2048, 4096):
        raise ImageError("block size must be 1024, 2048 or 4096")
    if inode_size not in (128, 256, 512):
        raise ImageError("inode size must be 128, 256 or 512")
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(output.name + ".tmp")
    if temporary.exists():
        temporary.unlink()
    with temporary.open("wb") as image_file:
        image_file.truncate(image_size_mb * 1024 * 1024)
    env = os.environ.copy()
    env["SOURCE_DATE_EPOCH"] = SOURCE_DATE_EPOCH
    env["E2FSPROGS_FAKE_TIME"] = SOURCE_DATE_EPOCH
    try:
        _run(["mke2fs", "-q", "-F", "-t", "ext4", "-b", str(block_size),
              "-I", str(inode_size), "-U", _fixed_uuid(arch, profile),
              "-L", _label(arch, profile), "-O", EXT4_FEATURES,
              "-E", "lazy_itable_init=0,lazy_journal_init=0",
              "-d", str(staging), str(temporary)], env=env)
        _normalize_image_metadata(temporary, staging, arch, profile)
        _validate_image(temporary, block_size, inode_size)
        os.replace(temporary, output)
    finally:
        if temporary.exists():
            temporary.unlink()
    manifest = staging_manifest(staging)
    _write_sidecars(output, manifest)
    print(f"[image] created: {output}")
    return output


def _debugfs_quote(value: str) -> str:
    if any(character in value for character in ("\0", "\n", "\r")):
        raise ImageError(f"unsupported control character in debugfs path: {value!r}")
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def _debugfs_stat(image: Path, logical: str) -> str | None:
    result = _run(["debugfs", "-R", f"stat {_debugfs_quote(logical)}", str(image)],
                  capture=True)
    combined = (result.stdout or "") + (result.stderr or "")
    if "File not found" in combined or "not found" in combined.lower():
        return None
    return combined


def _under(path: str, prefixes: Iterable[str]) -> bool:
    return any(path == prefix or path.startswith(prefix.rstrip("/") + "/")
               for prefix in prefixes)


def _copy_reflink(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    cp = shutil.which("cp")
    if cp:
        completed = subprocess.run([cp, "--reflink=auto", "--preserve=mode,timestamps",
                                    str(source), str(destination)])
        if completed.returncode == 0:
            return
    shutil.copy2(source, destination)


def create_overlay(staging: Path, base_image: Path, output: Path,
                   replace_prefixes: Iterable[str], arch: str, profile: str) -> Path:
    staging = staging.resolve()
    base_image = base_image.resolve()
    output = output.resolve()
    if not base_image.is_file():
        raise ImageError(f"base image does not exist: {base_image}")
    if base_image == output:
        raise ImageError("overlay output must differ from BASE_IMAGE")
    sidecars = (output.with_suffix(output.suffix + ".changes.json"),
                output.with_suffix(output.suffix + ".sha256"))
    output.unlink(missing_ok=True)
    for sidecar in sidecars:
        sidecar.unlink(missing_ok=True)
    base_checksum = _sha256(base_image)
    _copy_reflink(base_image, output)
    changes: list[dict[str, object]] = []
    commands: list[str] = []
    replace_prefixes = tuple(replace_prefixes)
    try:
        for entry in _iter_entries(staging):
            logical = "/" + entry.relative_to(staging).as_posix()
            if _under(logical, PROTECTED_PREFIXES):
                raise ImageError(f"package attempted to modify protected path: {logical}")
            structural_parent = (logical in OVERLAY_STRUCTURAL_PARENTS
                                 and entry.is_dir() and not entry.is_symlink())
            if not structural_parent and not _under(logical, OVERLAY_ALLOWED_PREFIXES):
                continue
            existing = _debugfs_stat(output, logical)
            replace = _under(logical, replace_prefixes)
            if entry.is_dir() and not entry.is_symlink():
                if existing is None:
                    commands.append(f"mkdir {_debugfs_quote(logical)}")
                    mode = "0" + format(0o040000 | (entry.stat().st_mode & 0o7777), "o")
                    commands.append(f"set_inode_field {_debugfs_quote(logical)} mode {mode}")
                    commands.extend(_inode_metadata_commands(logical))
                    changes.append({"path": logical, "action": "mkdir"})
                continue
            if existing is not None:
                if not replace:
                    raise ImageError(f"overlay path already exists without replacement permission: {logical}")
                commands.append(f"rm {_debugfs_quote(logical)}")
            if entry.is_symlink():
                target = os.readlink(entry)
                commands.append(f"symlink {_debugfs_quote(logical)} {_debugfs_quote(target)}")
                commands.extend(_inode_metadata_commands(logical))
                changes.append({"path": logical, "action": "replace" if existing else "symlink",
                                "target": target})
            else:
                commands.append(f"write {_debugfs_quote(str(entry))} {_debugfs_quote(logical)}")
                # debugfs `write` copies the host file's permission bits.  Do
                # not follow it with set_inode_field: that command expects an
                # e2fsprogs-internal numeric form and silently returns success
                # for some malformed values while leaving the mode unchanged.
                commands.extend(_inode_metadata_commands(logical))
                changes.append({"path": logical, "action": "replace" if existing else "write",
                                "sha256": _sha256(entry)})
        if commands:
            _run_debugfs_batch(output, commands)
        _run(["e2fsck", "-fn", str(output)])
        for change in changes:
            logical = str(change["path"])
            stat = _debugfs_stat(output, logical)
            if stat is None:
                raise ImageError(f"overlay write did not create {logical}")
            if not re.search(r"User:\s+0\s+Group:\s+0", stat):
                raise ImageError(f"overlay path is not owned by root:root: {logical}")
    except Exception:
        output.unlink(missing_ok=True)
        for sidecar in sidecars:
            sidecar.unlink(missing_ok=True)
        raise
    if _sha256(base_image) != base_checksum:
        output.unlink(missing_ok=True)
        raise ImageError("BASE_IMAGE changed while creating overlay")
    change_manifest = {
        "schema": 1, "architecture": arch, "profile": profile,
        "base_image": str(base_image), "base_sha256": base_checksum,
        "output_sha256": _sha256(output), "changes": changes,
    }
    changes_path = output.with_suffix(output.suffix + ".changes.json")
    changes_path.write_text(json.dumps(change_manifest, indent=2, sort_keys=True) + "\n",
                            encoding="utf-8")
    output.with_suffix(output.suffix + ".sha256").write_text(
        f"{change_manifest['output_sha256']}  {output.name}\n", encoding="utf-8")
    print(f"[image] overlay: {output}")
    print(f"[image] changes: {changes_path}")
    return output


def inspect_image(image: Path) -> None:
    image = image.resolve()
    if not image.is_file():
        raise ImageError(f"image does not exist: {image}")
    header = _run(["dumpe2fs", "-h", str(image)], capture=True).stdout
    wanted = ("Filesystem volume name:", "Filesystem UUID:", "Filesystem features:",
              "Block count:", "Block size:", "Inode size:")
    for line in header.splitlines():
        if line.startswith(wanted):
            print(line)
    metadata = _run(["debugfs", "-R", "cat /var/lib/wateros/packages.json", str(image)],
                    capture=True)
    print("\nEmbedded package metadata:")
    print(metadata.stdout.strip() or "(missing)")
    print(f"\nSHA-256: {_sha256(image)}")
