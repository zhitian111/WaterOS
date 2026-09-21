#!/usr/bin/env python3
"""Install pinned host-side cross toolchains used by the userland builder."""

from __future__ import annotations

import argparse
import hashlib
import os
import posixpath
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

# Ubuntu 24.04 提供完整的 LoongArch64 GNU 交叉编译器，但没有对应的 musl 包。
# setup 将这些 deb 解包到 user/build/toolchains/la，不修改宿主系统。最终用户程序
# 使用静态 glibc，因此运行时同样不依赖镜像中的动态加载器或共享库。
LA_DEBIAN_PACKAGES = (
    "binutils-loongarch64-linux-gnu",
    "cpp-14-loongarch64-linux-gnu",
    "gcc-14-loongarch64-linux-gnu",
    "gcc-14-loongarch64-linux-gnu-base",
    "libc6-loong64-cross",
    "libc6-dev-loong64-cross",
    "libgcc-s1-loong64-cross",
    "libgcc-14-dev-loong64-cross",
    "linux-libc-dev-loong64-cross",
)
LA_COMPILER_PREFIX = "loongarch64-linux-gnu-"
LA_ARCHLINUX_COMPILER_PREFIX = "loongarch64-unknown-linux-gnu-"


def release_for(architecture: str) -> ToolchainRelease:
    if architecture == "rv":
        return RV_RELEASE
    raise SetupError(f"{architecture} uses a managed Debian toolchain, not a tar release")


def _is_arch_linux() -> bool:
    try:
        return "ID=arch" in Path("/etc/os-release").read_text(encoding="utf-8")
    except OSError:
        return False


def validate_archlinux_loongarch_toolchain() -> str:
    """Validate the system toolchain supplied by Arch's GCC/libc package."""
    prefix = LA_ARCHLINUX_COMPILER_PREFIX
    required = tuple(f"{prefix}{suffix}"
                     for suffix in ("gcc", "ar", "ranlib", "strip", "readelf"))
    missing = [tool for tool in required if shutil.which(tool) is None]
    if missing:
        raise SetupError(
            "Arch LoongArch toolchain is incomplete; install "
            "loongarch64-linux-gnu-gcc-libc:\n  - " + "\n  - ".join(missing)
        )
    compiler = required[0]
    machine = subprocess.run([compiler, "-dumpmachine"], check=True, text=True,
                             capture_output=True).stdout.strip()
    if "loongarch64" not in machine:
        raise SetupError(f"installed compiler has unexpected target: {machine}")
    with tempfile.TemporaryDirectory(prefix="wateros-la-arch-toolchain-") as temporary:
        source = Path(temporary) / "probe.c"
        binary = Path(temporary) / "probe"
        source.write_text("int main(void) { return 0; }\n", encoding="utf-8")
        subprocess.run([compiler, "-mabi=lp64d", "-static", str(source),
                        "-o", str(binary)], check=True, capture_output=True)
    print(f"[setup] using Arch Linux system compiler prefix: {prefix}")
    return prefix


def _write_launcher(path: Path, body: str) -> None:
    path.write_text("#!/bin/sh\nset -eu\n" + body, encoding="utf-8")
    path.chmod(0o755)


def _loongarch_tool_root(destination: Path) -> Path:
    return destination / "usr/bin"


def create_loongarch_launchers(destination: Path) -> Path:
    """为解包后的 Ubuntu cross toolchain 创建可搬移的统一前缀。"""
    compiler = _loongarch_tool_root(destination) / "loongarch64-linux-gnu-gcc-14"
    if not compiler.is_file():
        raise SetupError(f"LoongArch compiler is missing after extraction: {compiler}")
    launchers = destination / "bin"
    launchers.mkdir(parents=True, exist_ok=True)
    prefix = launchers / LA_COMPILER_PREFIX
    _write_launcher(
        Path(f"{prefix}gcc"),
        'root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)\n'
        'export LD_LIBRARY_PATH="$root/usr/lib/x86_64-linux-gnu'
        '${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\n'
        'exec "$root/usr/bin/loongarch64-linux-gnu-gcc-14" '
        '--sysroot="$root" '
        '-B"$root/usr/lib/gcc-cross/loongarch64-linux-gnu/14/" '
        '-B"$root/usr/loongarch64-linux-gnu/bin/" "$@"\n',
    )
    # Makefile 会通过 CROSS_COMPILE 调用其中多个 binutils。全部创建 launcher，
    # 避免依赖宿主恰好安装了同名工具或解包目录中的绝对符号链接。
    for tool in ("ar", "as", "ld", "nm", "objcopy", "objdump", "ranlib",
                 "readelf", "size", "strings", "strip"):
        target = _loongarch_tool_root(destination) / f"loongarch64-linux-gnu-{tool}"
        if not target.exists():
            raise SetupError(f"LoongArch binutils tool is missing: {target}")
        _write_launcher(
            Path(f"{prefix}{tool}"),
            'root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)\n'
            'export LD_LIBRARY_PATH="$root/usr/lib/x86_64-linux-gnu'
            '${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\n'
            f'exec "$root/usr/bin/loongarch64-linux-gnu-{tool}" "$@"\n',
        )
    return prefix


def validate_loongarch_install(destination: Path) -> Path:
    prefix = create_loongarch_launchers(destination)
    compiler = Path(f"{prefix}gcc")
    result = subprocess.run([str(compiler), "-dumpmachine"], check=True,
                            text=True, capture_output=True).stdout.strip()
    if "loongarch64" not in result:
        raise SetupError(f"installed compiler has unexpected target: {result}")
    with tempfile.TemporaryDirectory(prefix="wateros-la-toolchain-") as temporary:
        shared = Path(temporary) / "probe.so"
        subprocess.run([str(compiler), "-shared", "-x", "c", "-", "-o", str(shared)],
                       input="int wateros_la_dynamic_probe;\n", text=True,
                       check=True, capture_output=True)
    return prefix


def install_loongarch_debian(force: bool) -> Path:
    """无 root 权限下载并解包 Ubuntu 的 LoongArch64 静态 GNU 工具链。"""
    destination = BUILD_ROOT / "toolchains/la"
    if destination.exists() and not force:
        try:
            prefix = validate_loongarch_install(destination)
            print(f"[setup] already installed: {destination}")
            return prefix
        except (SetupError, subprocess.CalledProcessError):
            raise SetupError(f"incomplete installation exists at {destination}; rerun with FORCE=1")
    if force:
        shutil.rmtree(destination, ignore_errors=True)
    apt_get = shutil.which("apt-get")
    dpkg_deb = shutil.which("dpkg-deb")
    if apt_get is None or dpkg_deb is None:
        raise SetupError(
            "automatic LoongArch setup requires Debian/Ubuntu apt-get and dpkg-deb; "
            "otherwise install a static cross toolchain and set LA_CROSS_COMPILE"
        )
    download_dir = BUILD_ROOT / "downloads/la-debian"
    download_dir.mkdir(parents=True, exist_ok=True)
    # apt-get download 无需 sudo，只把 deb 保存到当前目录，也不会修改宿主包数据库。
    print("[setup] download LoongArch cross packages without sudo", flush=True)
    subprocess.run([apt_get, "download", *LA_DEBIAN_PACKAGES],
                   cwd=download_dir, check=True)
    archives = sorted(download_dir.glob("*.deb"))
    if not archives:
        raise SetupError("apt-get did not download any LoongArch cross packages")
    destination.mkdir(parents=True)
    try:
        for archive in archives:
            print(f"[setup] extract: {archive.name}", flush=True)
            subprocess.run([dpkg_deb, "-x", str(archive), str(destination)], check=True)
        prefix = validate_loongarch_install(destination)
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise
    print(f"[setup] installed static glibc compiler prefix: {prefix}")
    return prefix


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
            extract_safely(bundle, temporary)
        extracted = temporary / archive_top_directory(archive)
        if not extracted.is_dir():
            raise SetupError("toolchain archive did not produce its declared root directory")
        shutil.move(str(extracted), str(destination))


def _safe_archive_path(path: str) -> bool:
    """Return whether a POSIX tar member path remains relative to its root."""
    normalized = posixpath.normpath(path)
    return not (path.startswith("/") or normalized in ("", ".", "..")
                or normalized.startswith("../"))


def extract_safely(bundle: tarfile.TarFile, destination: Path) -> None:
    """Extract with Python 3.11-compatible protections equivalent to data filter."""
    members = bundle.getmembers()
    for member in members:
        if not _safe_archive_path(member.name):
            raise SetupError(f"unsafe path in toolchain archive: {member.name!r}")
        if member.isdev() or member.isfifo():
            raise SetupError(f"unsafe special file in toolchain archive: {member.name!r}")
        if member.issym():
            target = posixpath.join(posixpath.dirname(member.name), member.linkname)
            if not _safe_archive_path(target):
                raise SetupError(f"unsafe symbolic link in toolchain archive: {member.name!r}")
        elif member.islnk() and not _safe_archive_path(member.linkname):
            raise SetupError(f"unsafe hard link in toolchain archive: {member.name!r}")
        # Match tarfile's data filter by dropping setuid, setgid and sticky bits.
        member.mode &= 0o755

    if sys.version_info >= (3, 12):
        # Python's built-in filter also protects against link traversal.
        bundle.extractall(destination, filter="data")
    else:
        # The validation above preserves support for Python 3.11, whose tarfile
        # module predates the filter= API.
        bundle.extractall(destination)


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
        if args.arch == "la":
            if args.archive is not None:
                raise SetupError("TOOLCHAIN_ARCHIVE is currently supported only for ARCH=rv")
            if _is_arch_linux():
                validate_archlinux_loongarch_toolchain()
            else:
                install_loongarch_debian(args.force)
        else:
            install(release_for(args.arch), args.archive, args.force)
        return 0
    except (SetupError, OSError, tarfile.TarError, subprocess.CalledProcessError) as error:
        print(f"[setup] error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
