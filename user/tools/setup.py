#!/usr/bin/env python3
"""Install pinned host-side cross toolchains used by the userland builder."""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path


USER_ROOT = Path(__file__).resolve().parents[1]
BUILD_ROOT = USER_ROOT / "build"


class SetupError(RuntimeError):
    """A deterministic download, verification or installation error."""


@dataclass(frozen=True)
class ToolchainRelease:
    architecture: str
    archive_name: str
    url: str
    sha256: str
    compiler_prefix: str


RV_RELEASE = ToolchainRelease(
    architecture="rv",
    archive_name="riscv64-lp64d--musl--stable-2025.08-1.tar.xz",
    url=("https://toolchains.bootlin.com/downloads/releases/toolchains/"
         "riscv64-lp64d/tarballs/"
         "riscv64-lp64d--musl--stable-2025.08-1.tar.xz"),
    sha256="2c5155ce133c9c8dddde8f69b0715aa07e0520d99b1fd0131d915357c6fbce39",
    compiler_prefix="riscv64-buildroot-linux-musl-",
)


def release_for(architecture: str) -> ToolchainRelease:
    if architecture == "rv":
        return RV_RELEASE
    raise SetupError(
        "no pinned LoongArch musl binary toolchain is available yet; "
        "install one separately and set LA_CROSS_COMPILE=/path/to/prefix-"
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_archive(path: Path, release: ToolchainRelease) -> None:
    actual = sha256_file(path)
    if actual != release.sha256:
        raise SetupError(
            f"toolchain checksum mismatch for {path}\n"
            f"expected: {release.sha256}\nactual:   {actual}"
        )


def download(release: ToolchainRelease, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".part")
    temporary.unlink(missing_ok=True)
    print(f"[setup] download: {release.url}", flush=True)
    try:
        request = urllib.request.Request(release.url, headers={"User-Agent": "WaterOS-userland/1"})
        with urllib.request.urlopen(request) as response, temporary.open("wb") as output:
            total = int(response.headers.get("Content-Length", "0"))
            received = 0
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                output.write(chunk)
                received += len(chunk)
                if total:
                    print(f"\r[setup] {received * 100 // total:3d}% "
                          f"({received // (1024 * 1024)}/{total // (1024 * 1024)} MiB)",
                          end="", flush=True)
            if total:
                print()
        os.replace(temporary, destination)
    except (OSError, urllib.error.URLError) as error:
        temporary.unlink(missing_ok=True)
        raise SetupError(f"cannot download toolchain: {error}") from error


def archive_top_directory(archive: Path) -> str:
    with tarfile.open(archive, "r:xz") as bundle:
        roots = {Path(member.name).parts[0] for member in bundle.getmembers()
                 if member.name and Path(member.name).parts}
    if len(roots) != 1:
        raise SetupError(f"toolchain archive must contain one top-level directory: {sorted(roots)}")
    return roots.pop()


def extract_archive(archive: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".toolchain-install-",
                                     dir=destination.parent) as temporary_text:
        temporary = Path(temporary_text)
        print(f"[setup] extract: {archive}", flush=True)
        with tarfile.open(archive, "r:xz") as bundle:
            # Python 3.12's data filter rejects absolute paths, traversal and
            # unsafe special files before anything reaches the filesystem.
            bundle.extractall(temporary, filter="data")
        extracted = temporary / archive_top_directory(archive)
        if not extracted.is_dir():
            raise SetupError("toolchain archive did not produce its declared root directory")
        shutil.move(str(extracted), str(destination))


def validate_install(destination: Path, release: ToolchainRelease) -> Path:
    prefix = destination / "bin" / release.compiler_prefix
    required = tuple(Path(f"{prefix}{suffix}") for suffix in ("gcc", "ar", "strip"))
    missing = [str(path) for path in required if not path.is_file()]
    if missing:
        raise SetupError("installed toolchain is incomplete:\n  - " + "\n  - ".join(missing))
    result = subprocess.run([str(required[0]), "-dumpmachine"], check=True,
                            text=True, capture_output=True).stdout.strip()
    if "riscv64" not in result or "musl" not in result:
        raise SetupError(f"installed compiler has unexpected target: {result}")
    return prefix


def install(release: ToolchainRelease, archive: Path | None, force: bool) -> Path:
    destination = BUILD_ROOT / "toolchains" / release.architecture
    if destination.exists() and not force:
        try:
            prefix = validate_install(destination, release)
            print(f"[setup] already installed: {destination}")
            return prefix
        except (SetupError, subprocess.CalledProcessError):
            raise SetupError(f"incomplete installation exists at {destination}; rerun with FORCE=1")
    if force:
        shutil.rmtree(destination, ignore_errors=True)

    if archive is None:
        archive = BUILD_ROOT / "downloads" / release.archive_name
        if not archive.is_file():
            download(release, archive)
    archive = archive.expanduser().resolve()
    if not archive.is_file():
        raise SetupError(f"toolchain archive does not exist: {archive}")
    print(f"[setup] verify SHA-256: {archive}", flush=True)
    verify_archive(archive, release)

    try:
        extract_archive(archive, destination)
        relocation = destination / "relocate-sdk.sh"
        if relocation.is_file():
            print(f"[setup] relocate SDK: {destination}", flush=True)
            subprocess.run([str(relocation)], cwd=destination, check=True)
        prefix = validate_install(destination, release)
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise
    print(f"[setup] installed compiler prefix: {prefix}")
    return prefix


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=("rv", "la"), required=True)
    parser.add_argument("--archive", type=Path,
                        help="use the pinned archive from a local/offline path")
    parser.add_argument("--force", action="store_true",
                        help="replace an incomplete or existing managed toolchain")
    return parser.parse_args()


def main() -> int:
    args = parse_arguments()
    try:
        install(release_for(args.arch), args.archive, args.force)
        return 0
    except (SetupError, OSError, tarfile.TarError, subprocess.CalledProcessError) as error:
        print(f"[setup] error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
