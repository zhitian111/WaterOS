#!/usr/bin/env python3
"""Cross-build a small, static pacman for the musl RISC-V WaterOS image.

Sources are fetched into ``user/build/downloads/pacman`` and verified before
use.  Nothing downloaded by this script is a repository input or should be
committed.  The deliberately small first build supports local ``pacman -U``;
curl and GPGME are left disabled until their WaterOS runtime paths are tested.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import tarfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


@dataclass(frozen=True)
class Source:
    name: str
    url: str
    sha256: str


SOURCES = (
    Source("zlib-1.3.1.tar.gz",
           "https://github.com/madler/zlib/archive/refs/tags/v1.3.1.tar.gz",
           "17e88863f3600672ab49182f217281b6fc4d3c762bde361935e436a95214d05c"),
    Source("xz-5.6.3.tar.gz",
           "https://github.com/tukaani-project/xz/releases/download/v5.6.3/xz-5.6.3.tar.gz",
           "b1d45295d3f71f25a4c9101bd7c8d16cb56348bbef3bbc738da0351e17c73317"),
    Source("zstd-1.5.7.tar.gz",
           "https://github.com/facebook/zstd/releases/download/v1.5.7/zstd-1.5.7.tar.gz",
           "eb33e51f49a15e023950cd7825ca74a4a2b43db8354825ac24fc1b7ee09e6fa3"),
    Source("openssl-3.3.2.tar.gz",
           "https://github.com/openssl/openssl/releases/download/openssl-3.3.2/openssl-3.3.2.tar.gz",
           "2e8a40b01979afe8be0bbfb3de5dc1c6709fedb46d6c89c10da114ab5fc3d281"),
    Source("curl-8.10.1.tar.xz",
           "https://curl.se/download/curl-8.10.1.tar.xz",
           "73a4b0e99596a09fa5924a4fb7e4b995a85fda0d18a2c02ab9cf134bebce04ee"),
    Source("libarchive-3.7.7.tar.xz",
           "https://github.com/libarchive/libarchive/releases/download/v3.7.7/libarchive-3.7.7.tar.xz",
           "879acd83c3399c7caaee73fe5f7418e06087ab2aaf40af3e99b9e29beb29faee"),
    Source("pacman-v7.0.0.tar.gz",
           "https://gitlab.archlinux.org/pacman/pacman/-/archive/v7.0.0/pacman-v7.0.0.tar.gz",
           "ef08f258cb3e0885c5884ad43fb6cff0e9c327ed33024d79d03555f99c583744"),
    Source("ca-certificates-bundle-20260611-r0.apk",
           "https://dl-cdn.alpinelinux.org/alpine/v3.22/main/riscv64/ca-certificates-bundle-20260611-r0.apk",
           "537dcb625ede1cb81e751dd92552b2715a35fdd72cdb43a965a055f14900d529"),
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fetch(source: Source, downloads: Path) -> Path:
    result = downloads / source.name
    if result.is_file() and sha256(result) == source.sha256:
        print(f"[pacman] cache hit: {source.name}", flush=True)
        return result
    result.unlink(missing_ok=True)
    temporary = result.with_suffix(result.suffix + ".part")
    temporary.unlink(missing_ok=True)
    print(f"[pacman] download: {source.url}", flush=True)
    request = urllib.request.Request(source.url, headers={"User-Agent": "WaterOS-userland/1"})
    try:
        with urllib.request.urlopen(request) as response, temporary.open("wb") as output:
            shutil.copyfileobj(response, output, 1024 * 1024)
        actual = sha256(temporary)
        if actual != source.sha256:
            raise RuntimeError(f"checksum mismatch for {source.name}: {actual}")
        os.replace(temporary, result)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise
    return result


def unpack(archive: Path, destination: Path) -> Path:
    with tarfile.open(archive) as bundle:
        members = bundle.getmembers()
        roots = {PurePosixPath(member.name).parts[0] for member in members if member.name}
        if len(roots) != 1:
            raise RuntimeError(f"{archive.name} has unexpected top-level entries")
        for member in members:
            path = PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts or member.isdev() or member.isfifo():
                raise RuntimeError(f"unsafe archive member: {member.name!r}")
        bundle.extractall(destination, filter="data")
    return destination / roots.pop()


def run(command: list[str], *, cwd: Path, env: dict[str, str]) -> None:
    print("[pacman]", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def arch_linux_uapi_flags(context: dict) -> list[str]:
    """Supply Arch's separately installed RISC-V Linux UAPI headers."""
    headers = Path("/usr/riscv64-linux-gnu/include/linux/mman.h")
    if context["arch"] == "rv" and headers.is_file():
        return ["-isystem", "/usr/riscv64-linux-gnu/include"]
    return []


def install_musl_loader(destination: Path) -> None:
    """Install the loader used by the Arch musl compiler wrapper.

    ``riscv64-linux-musl-gcc`` deliberately emits this interpreter path.  The
    static third-party libraries do not need to be installed, but pacman and
    its shared libalpm do need this one musl ELF image at runtime.
    """
    source = Path("/usr/riscv64-linux-musl/lib/musl/lib/libc.so")
    if not source.is_file():
        raise RuntimeError(
            "dynamic pacman needs the musl loader; install the Arch musl wrapper "
            "or use a toolchain that provides /lib/ld-musl-riscv64.so.1"
        )
    target = destination / "lib/ld-musl-riscv64.so.1"
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)


def install_ca_bundle(archive: Path, destination: Path) -> None:
    """Extract only the public CA bundle from Alpine's signed package payload."""
    member_name = "etc/ssl/certs/ca-certificates.crt"
    with tarfile.open(archive, "r:gz") as bundle:
        member = bundle.getmember(member_name)
        source = bundle.extractfile(member)
        if source is None:
            raise RuntimeError("CA bundle archive has no certificate payload")
        target = destination / member_name
        target.parent.mkdir(parents=True, exist_ok=True)
        with source, target.open("wb") as output:
            shutil.copyfileobj(source, output)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True, type=Path)
    args = parser.parse_args()
    context = json.loads(args.context.read_text(encoding="utf-8"))
    if context["arch"] != "rv":
        raise RuntimeError("pacman is currently packaged only for riscv64-musl")

    root = Path(context["user_root"])
    package = Path(context["package_dir"])
    destination = Path(context["destdir"])
    downloads = root / "build/downloads/pacman"
    work = Path(context["work_dir"])
    prefix = work / "prefix"
    downloads.mkdir(parents=True, exist_ok=True)
    source_root = work / "sources"
    source_root.mkdir(parents=True, exist_ok=True)
    archives = {source.name: fetch(source, downloads) for source in SOURCES}
    trees = {name: unpack(archive, source_root) for name, archive in archives.items()
             if name.endswith((".tar.gz", ".tar.xz"))}

    cross = context["cross_compile"]
    env = os.environ.copy()
    uapi_flags = arch_linux_uapi_flags(context)
    cflags = context["cflags"] + ["-O2", "-fPIC", *uapi_flags]
    env.update(CC=f"{cross}gcc", AR=f"{cross}ar", RANLIB=f"{cross}ranlib",
               STRIP=f"{cross}strip", CFLAGS=" ".join(context["cflags"] + ["-O2", "-fPIC"]),
               CPPFLAGS=f"-I{prefix}/include", LDFLAGS=f"-L{prefix}/lib",
               PKG_CONFIG_PATH=f"{prefix}/lib/pkgconfig")
    env["CFLAGS"] = " ".join(cflags)
    jobs = str(context["jobs"])

    zlib = trees["zlib-1.3.1.tar.gz"]
    run(["cmake", "-S", str(zlib), "-B", str(work / "zlib-build"),
         f"-DCMAKE_C_COMPILER={cross}gcc", f"-DCMAKE_AR={cross}ar",
         f"-DCMAKE_RANLIB={cross}ranlib", "-DBUILD_SHARED_LIBS=OFF",
         f"-DCMAKE_INSTALL_PREFIX={prefix}"], cwd=work, env=env)
    run(["cmake", "--build", str(work / "zlib-build"), "-j", jobs], cwd=work, env=env)
    run(["cmake", "--install", str(work / "zlib-build")], cwd=work, env=env)

    zstd = trees["zstd-1.5.7.tar.gz"] / "build/cmake"
    run(["cmake", "-S", str(zstd), "-B", str(work / "zstd-build"),
         f"-DCMAKE_C_COMPILER={cross}gcc", f"-DCMAKE_AR={cross}ar",
         f"-DCMAKE_RANLIB={cross}ranlib", "-DZSTD_BUILD_SHARED=OFF",
         "-DZSTD_BUILD_PROGRAMS=OFF", "-DZSTD_BUILD_TESTS=OFF",
         f"-DCMAKE_INSTALL_PREFIX={prefix}"], cwd=work, env=env)
    run(["cmake", "--build", str(work / "zstd-build"), "-j", jobs], cwd=work, env=env)
    run(["cmake", "--install", str(work / "zstd-build")], cwd=work, env=env)

    xz = trees["xz-5.6.3.tar.gz"]
    run(["./configure", "--host=riscv64-linux-musl", f"--prefix={prefix}",
         "--disable-shared", "--enable-static", "--disable-nls"], cwd=xz, env=env)
    run(["make", f"-j{jobs}"], cwd=xz, env=env)
    run(["make", "install"], cwd=xz, env=env)

    openssl = trees["openssl-3.3.2.tar.gz"]
    run(["./Configure", "linux64-riscv64", "no-shared", "no-tests", "no-apps",
         f"--prefix={prefix}"], cwd=openssl, env=env)
    run(["make", f"-j{jobs}"], cwd=openssl, env=env)
    run(["make", "install_sw"], cwd=openssl, env=env)

    curl = trees["curl-8.10.1.tar.xz"]
    run(["./configure", "--host=riscv64-linux-musl", f"--prefix={prefix}",
         "--disable-shared", "--enable-static", f"--with-openssl={prefix}",
         f"--with-zlib={prefix}", "--without-brotli", "--without-zstd",
         "--without-libpsl", "--disable-ldap", "--disable-ldaps", "--disable-rtsp",
         "--disable-dict", "--disable-telnet", "--disable-tftp", "--disable-pop3",
         "--disable-imap", "--disable-smtp", "--disable-gopher", "--disable-mqtt",
         "--disable-manual", "--disable-libcurl-option", "--disable-sspi",
         "--with-ca-bundle=/etc/ssl/certs/ca-certificates.crt"], cwd=curl, env=env)
    run(["make", f"-j{jobs}"], cwd=curl, env=env)
    run(["make", "install"], cwd=curl, env=env)

    libarchive = trees["libarchive-3.7.7.tar.xz"]
    run(["cmake", "-S", str(libarchive), "-B", str(work / "libarchive-build"),
         f"-DCMAKE_C_COMPILER={cross}gcc", f"-DCMAKE_AR={cross}ar",
         f"-DCMAKE_RANLIB={cross}ranlib", "-DBUILD_SHARED_LIBS=OFF",
         "-DENABLE_TAR=OFF", "-DENABLE_CPIO=OFF", "-DENABLE_CAT=OFF",
         "-DENABLE_TEST=OFF", "-DENABLE_BZip2=OFF", "-DENABLE_LZ4=OFF",
         "-DENABLE_LZO=OFF", "-DENABLE_LIBB2=OFF", "-DENABLE_LIBXML2=OFF",
         "-DENABLE_EXPAT=OFF", "-DENABLE_ICONV=OFF", "-DENABLE_ACL=OFF",
         "-DCMAKE_POLICY_VERSION_MINIMUM=3.5",
         f"-DCMAKE_PREFIX_PATH={prefix}", f"-DCMAKE_INSTALL_PREFIX={prefix}"],
        cwd=work, env=env)
    run(["cmake", "--build", str(work / "libarchive-build"), "-j", jobs], cwd=work, env=env)
    run(["cmake", "--install", str(work / "libarchive-build")], cwd=work, env=env)

    meson_c_args = context["cflags"] + ["-O2", *uapi_flags]
    cross_file = work / "meson-cross.ini"
    cross_file.write_text(
        "[binaries]\n"
        f"c = '{cross}gcc'\n"
        f"ar = '{cross}ar'\n"
        f"strip = '{cross}strip'\n"
        "pkg-config = 'pkg-config'\n\n"
        "[host_machine]\n"
        "system = 'linux'\n"
        "cpu_family = 'riscv64'\n"
        "cpu = 'riscv64'\n"
        "endian = 'little'\n\n"
        "[properties]\n"
        f"c_args = {meson_c_args!r}\n"
        f"c_link_args = ['-L{prefix}/lib']\n",
        encoding="utf-8")
    pacman = trees["pacman-v7.0.0.tar.gz"]
    pacman_build = work / "pacman-build"
    run(["meson", "setup", str(pacman_build), str(pacman), "--cross-file", str(cross_file),
         "-Dbuildstatic=true", "-Dcurl=enabled", "-Dgpgme=disabled", "-Di18n=false",
         "-Ddoc=disabled", "-Dcrypto=openssl", "-Dpkg-ext=.pkg.tar.zst"], cwd=work, env=env)
    run(["meson", "compile", "-C", str(pacman_build), "-j", jobs], cwd=work, env=env)
    run(["meson", "install", "-C", str(pacman_build), f"--destdir={destination}"], cwd=work, env=env)

    for relative in ("var/lib/pacman/local", "var/lib/pacman/sync", "var/cache/pacman/pkg",
                     "etc/pacman.d/hooks"):
        (destination / relative).mkdir(parents=True, exist_ok=True)
    install_musl_loader(destination)
    install_ca_bundle(archives["ca-certificates-bundle-20260611-r0.apk"], destination)
    shutil.copy2(package / "assets/pacman.conf", destination / "etc/pacman.conf")
    mirrorlist = destination / "etc/pacman.d/mirrorlist"
    mirrorlist.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(package / "assets/mirrorlist", mirrorlist)
    for name in ("archriscv-pacman", "archriscv-run"):
        target = destination / "usr/bin" / name
        shutil.copy2(package / "assets" / name, target)
        target.chmod(0o755)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
