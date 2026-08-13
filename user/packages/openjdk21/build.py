#!/usr/bin/env python3
"""Install pinned OpenJDK 21 headless runtimes for WaterOS."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import shutil
import stat
import subprocess
import tarfile
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


ALPINE_RELEASE = "v3.22"
ALPINE_BASE = f"https://dl-cdn.alpinelinux.org/alpine/{ALPINE_RELEASE}"
LOONGSON_JDK_FILENAME = (
    "loongson21.11.38-fx-jdk21.0.11_10-linux-loongarch64-glibc2.34.tar.gz"
)
LOONGSON_JDK_URL = f"https://ftp.loongnix.cn/Java/openjdk21/{LOONGSON_JDK_FILENAME}"
LOONGSON_JDK_SHA256 = "3fd0b56e2e060d668f5392c9fb9f2c6f243b4fcc6daaea01dabfdea6e5d153fb"
ZLIB_FILENAME = "zlib-1.3.2.tar.gz"
ZLIB_URL = f"https://github.com/madler/zlib/releases/download/v1.3.2/{ZLIB_FILENAME}"
ZLIB_SHA256 = "bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16"


@dataclass(frozen=True)
class Archive:
    repository: str
    filename: str
    sha256: str

    @property
    def url(self) -> str:
        return f"{ALPINE_BASE}/{self.repository}/riscv64/{self.filename}"


ARCHIVES = (
    Archive("community", "openjdk21-jre-headless-21.0.11_p10-r0.apk",
            "41e1a3a1234c6cf5014d46288ed0d3c0b475e162d1384e42018eb13fbf47726c"),
    Archive("main", "musl-1.2.5-r12.apk",
            "6814d9cbaad929d14181ef4fbd1d65c7749df43746269b9bdb75551ba32a79db"),
    Archive("main", "zlib-1.3.2-r0.apk",
            "9a2761a457312f4aa1312c94d3ca8789c2f1dd51d34d992e400851c8181a6887"),
    Archive("main", "ca-certificates-bundle-20260611-r0.apk",
            "537dcb625ede1cb81e751dd92552b2715a35fdd72cdb43a965a055f14900d529"),
)

JAVA_CACERTS_SHA256 = "d8688143c6107456a13d959ae23c9f375ee2b743c8bb5f59be77d9b5ac956173"

TEST_CLASSES = {
    "Hello": "906724e222f455d84247d87ac28fadcc1e0063901ed6586db3e83808e0b3e9b1",
    "RuntimeProbe": "d630f995f6a12c8b488fd9f4de1f2f338f7910c4679f285a163ef342bde1c9e1",
    "NetworkProbe": "5a4d9b50a02b01302399667bbf5cf731c511a2fa6b72dc3785642be2c5ed38d0",
    "ExceptionProbe": "4a582d516923a72f31e0e838217646b8a64e5b277248c6556cb35ee18f8d0b95",
    "JitProbe": "8f5cd72a5051fdf1f904ff76b217467665010721705126cb9b741ba2f965d169",
}

TEST_JARS = {
    "ApplicationProbe": "94034130a5be3970f06739c6653922b77e4652d82af1f660752d416236e51c28",
}

HEADLESS_BINARIES = (
    "java", "jfr", "jrunscript", "jwebserver", "keytool", "rmiregistry",
)
HEADLESS_LIBRARIES = (
    "classlist", "javafx.properties", "jexec", "jrt-fs.jar", "jspawnhelper",
    "jvm.cfg", "libattach.so", "libawt.so", "libawt_headless.so",
    "libdt_socket.so", "libextnet.so", "libinstrument.so", "libj2gss.so",
    "libj2pcsc.so", "libj2pkcs11.so", "libjaas.so", "libjava.so",
    "libjdwp.so", "libjimage.so", "libjli.so", "libjsig.so", "lible.so",
    "libmanagement.so", "libmanagement_agent.so", "libmanagement_ext.so",
    "libmlib_image.so", "libnet.so", "libnio.so", "libprefs.so", "librmi.so",
    "libsaproc.so", "libsctp.so", "libsyslookup.so", "libverify.so", "libzip.so",
    "modules", "psfont.properties.ja", "psfontj2d.properties", "tzdb.dat",
)
GLIBC_RUNTIME_LIBRARIES = (
    "ld-linux-loongarch-lp64d.so.1", "libc.so.6", "libdl.so.2", "libm.so.6",
    "libpthread.so.0", "libresolv.so.2", "librt.so.1", "libnss_dns.so.2",
    "libnss_files.so.2", "libgcc_s.so.1",
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download_url(url: str, sha256: str, destination: Path) -> None:
    if destination.is_file() and sha256_file(destination) == sha256:
        print(f"[openjdk21] cache hit: {destination.name}", flush=True)
        return
    destination.unlink(missing_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".part")
    temporary.unlink(missing_ok=True)
    print(f"[openjdk21] download: {url}", flush=True)
    request = urllib.request.Request(url,
                                     headers={"User-Agent": "WaterOS-userland/1"})
    try:
        with urllib.request.urlopen(request) as response, temporary.open("wb") as output:
            shutil.copyfileobj(response, output, 1024 * 1024)
        actual = sha256_file(temporary)
        if actual != sha256:
            raise RuntimeError(
                f"checksum mismatch for {destination.name}: "
                f"expected {sha256}, got {actual}"
            )
        os.replace(temporary, destination)
    except (OSError, urllib.error.URLError):
        temporary.unlink(missing_ok=True)
        raise


def download(archive: Archive, destination: Path) -> None:
    download_url(archive.url, archive.sha256, destination)


def safe_member_path(name: str) -> Path:
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or not path.parts:
        raise RuntimeError(f"unsafe APK member path: {name!r}")
    return Path(*path.parts)


def ensure_real_parent(root: Path, relative: Path) -> Path:
    current = root
    for part in relative.parent.parts:
        current /= part
        if current.is_symlink():
            raise RuntimeError(f"APK member traverses a symlink: {relative}")
        current.mkdir(exist_ok=True)
    return root / relative


def extract_payload(archive: Path, destination: Path) -> None:
    """Extract APK data without installing its signature/control members."""
    with tarfile.open(archive, "r:gz") as bundle:
        for member in bundle.getmembers():
            relative = safe_member_path(member.name)
            if relative.parts[0].startswith("."):
                continue
            target = ensure_real_parent(destination, relative)
            mode = member.mode & 0o7777
            if member.isdir():
                if target.is_symlink() or (target.exists() and not target.is_dir()):
                    raise RuntimeError(f"APK directory conflicts with {relative}")
                target.mkdir(exist_ok=True)
                target.chmod(mode)
            elif member.isfile():
                if target.exists() or target.is_symlink():
                    target.unlink()
                source = bundle.extractfile(member)
                if source is None:
                    raise RuntimeError(f"cannot read APK member: {relative}")
                with source, target.open("wb") as output:
                    shutil.copyfileobj(source, output, 1024 * 1024)
                target.chmod(mode)
            elif member.issym():
                if target.exists() or target.is_symlink():
                    target.unlink()
                target.symlink_to(member.linkname)
            elif member.islnk():
                link_source = destination / safe_member_path(member.linkname)
                if not link_source.is_file() or link_source.is_symlink():
                    raise RuntimeError(f"unsafe APK hard link: {relative}")
                if target.exists() or target.is_symlink():
                    target.unlink()
                os.link(link_source, target)
            else:
                raise RuntimeError(f"unsupported APK member type: {relative}")


def extract_tar_tree(archive: Path, destination: Path) -> None:
    """Extract a regular tar tree while rejecting paths outside DESTDIR."""
    with tarfile.open(archive, "r:gz") as bundle:
        for member in bundle.getmembers():
            relative = safe_member_path(member.name)
            target = ensure_real_parent(destination, relative)
            mode = member.mode & 0o7777
            if member.isdir():
                target.mkdir(exist_ok=True)
                target.chmod(mode)
            elif member.isfile():
                source = bundle.extractfile(member)
                if source is None:
                    raise RuntimeError(f"cannot read tar member: {relative}")
                with source, target.open("wb") as output:
                    shutil.copyfileobj(source, output, 1024 * 1024)
                target.chmod(mode)
            elif member.issym():
                if target.exists() or target.is_symlink():
                    target.unlink()
                target.symlink_to(member.linkname)
            else:
                raise RuntimeError(f"unsupported tar member type: {relative}")


def copy_required(source: Path, target: Path) -> None:
    if not source.exists() and not source.is_symlink():
        raise RuntimeError(f"required runtime file is missing: {source}")
    target.parent.mkdir(parents=True, exist_ok=True)
    if source.is_dir():
        shutil.copytree(source, target, symlinks=True)
    else:
        shutil.copy2(source, target, follow_symlinks=True)


def build_loongarch_zlib(archive: Path, context: dict, destination: Path) -> None:
    source = Path(context["work_dir"]) / "zlib"
    source.mkdir(parents=True, exist_ok=True)
    extract_tar_tree(archive, source)
    entries = [entry for entry in source.iterdir() if entry.is_dir()]
    if len(entries) != 1:
        raise RuntimeError("zlib archive must contain one top-level directory")
    build = entries[0]
    environment = os.environ.copy()
    cross = context["cross_compile"]
    environment.update(CHOST="loongarch64-linux-gnu", CC=f"{cross}gcc",
                       AR=f"{cross}ar", RANLIB=f"{cross}ranlib")
    subprocess.run(["./configure", "--prefix=/usr"], cwd=build,
                   env=environment, check=True)
    subprocess.run(["make", f"-j{context['jobs']}"], cwd=build,
                   env=environment, check=True)
    copy_required(build / "libz.so.1.3.2", destination / "usr/lib/libz.so.1.3.2")
    (destination / "usr/lib/libz.so.1").symlink_to("libz.so.1.3.2")


def install_loongarch_glibc(context: dict, destination: Path) -> None:
    compiler = f"{context['cross_compile']}gcc"
    sysroot = Path(subprocess.run([compiler, "-print-sysroot"], check=True,
                                  text=True, capture_output=True).stdout.strip())
    target_lib = sysroot / "usr/loongarch64-linux-gnu/lib"
    for name in GLIBC_RUNTIME_LIBRARIES:
        copy_required(target_lib / name, destination / "lib" / name)
    lib64 = destination / "lib64"
    lib64.mkdir(parents=True, exist_ok=True)
    (lib64 / "ld-linux-loongarch-lp64d.so.1").symlink_to(
        "../lib/ld-linux-loongarch-lp64d.so.1"
    )
    (destination / "lib/ld.so.1").symlink_to("ld-linux-loongarch-lp64d.so.1")


def install_loongarch_jdk(archive: Path, context: dict, destination: Path) -> None:
    extracted = Path(context["work_dir"]) / "loongson-jdk"
    extracted.mkdir(parents=True, exist_ok=True)
    extract_tar_tree(archive, extracted)
    source = extracted / "jdk-21.0.11"
    target = destination / "usr/lib/jvm/java-21-openjdk"
    target.mkdir(parents=True, exist_ok=True)
    copy_required(source / "release", target / "release")
    for directory in ("conf", "legal"):
        copy_required(source / directory, target / directory)
    for name in HEADLESS_BINARIES:
        copy_required(source / "bin" / name, target / "bin" / name)
    for name in HEADLESS_LIBRARIES:
        copy_required(source / "lib" / name, target / "lib" / name)
    for directory in ("jfr", "security", "server"):
        copy_required(source / "lib" / directory, target / "lib" / directory)


def install_test_classes(package: Path, destination: Path) -> None:
    tests = destination / "opt/wateros/jvm-tests"
    tests.mkdir(parents=True)
    for name, expected in TEST_CLASSES.items():
        encoded = (package / f"tests/{name}.class.b64").read_text(encoding="ascii")
        content = base64.b64decode(encoded, validate=False)
        actual = hashlib.sha256(content).hexdigest()
        if actual != expected:
            raise RuntimeError(f"test class checksum mismatch for {name}: {actual}")
        (tests / f"{name}.class").write_bytes(content)
        shutil.copy2(package / f"tests/{name}.java", tests / f"{name}.java")
    for name, expected in TEST_JARS.items():
        encoded = (package / f"tests/{name}.jar.b64").read_text(encoding="ascii")
        content = base64.b64decode(encoded, validate=False)
        actual = hashlib.sha256(content).hexdigest()
        if actual != expected:
            raise RuntimeError(f"test JAR checksum mismatch for {name}: {actual}")
        (tests / f"{name}.jar").write_bytes(content)
        shutil.copy2(package / f"tests/{name}.java", tests / f"{name}.java")
    shutil.copy2(package / "tests/probe-resource.txt", tests / "probe-resource.txt")


def install_java_truststore(package: Path, destination: Path) -> None:
    source = package / "assets/cacerts"
    actual = sha256_file(source)
    if actual != JAVA_CACERTS_SHA256:
        raise RuntimeError(
            f"Java truststore checksum mismatch: expected {JAVA_CACERTS_SHA256}, got {actual}"
        )
    target = destination / "etc/ssl/certs/java/cacerts"
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
    target.chmod(0o644)


def validate_elf(binary: Path, readelf: str, architecture: str) -> None:
    header = subprocess.run([readelf, "-h", str(binary)], check=True,
                            text=True, capture_output=True).stdout
    program = subprocess.run([readelf, "-l", str(binary)], check=True,
                             text=True, capture_output=True).stdout
    dynamic = subprocess.run([readelf, "-d", str(binary)], check=True,
                             text=True, capture_output=True).stdout
    if architecture == "rv":
        machine = "risc-v"
        interpreter = "/lib/ld-musl-riscv64.so.1"
        dependencies = ("libjli.so", "libc.musl-riscv64.so.1")
    else:
        machine = "loongarch"
        interpreter = "/lib64/ld-linux-loongarch-lp64d.so.1"
        dependencies = ("libz.so.1", "libjli.so", "libc.so.6")
    if machine not in header.lower():
        raise RuntimeError(f"OpenJDK launcher is not a {machine} ELF")
    if interpreter not in program:
        raise RuntimeError(f"OpenJDK launcher does not use {interpreter}")
    for dependency in dependencies:
        if dependency not in dynamic:
            raise RuntimeError(f"OpenJDK launcher is missing dependency {dependency}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--context", required=True, type=Path)
    args = parser.parse_args()
    context = json.loads(args.context.read_text(encoding="utf-8"))
    package = Path(context["package_dir"])
    destination = Path(context["destdir"])
    downloads = Path(context["user_root"]) / "build/downloads/openjdk21"
    downloads.mkdir(parents=True, exist_ok=True)
    if context["arch"] == "rv":
        for archive in ARCHIVES:
            local = downloads / archive.filename
            download(archive, local)
            extract_payload(local, destination)
    else:
        jdk = downloads / LOONGSON_JDK_FILENAME
        zlib = downloads / ZLIB_FILENAME
        ca_bundle = downloads / ARCHIVES[-1].filename
        download_url(LOONGSON_JDK_URL, LOONGSON_JDK_SHA256, jdk)
        download_url(ZLIB_URL, ZLIB_SHA256, zlib)
        download(ARCHIVES[-1], ca_bundle)
        extract_payload(ca_bundle, destination)
        install_loongarch_jdk(jdk, context, destination)
        install_loongarch_glibc(context, destination)
        build_loongarch_zlib(zlib, context, destination)

    jvm_root = destination / "usr/lib/jvm"
    default_jvm = jvm_root / "default-jvm"
    default_jvm.symlink_to("java-21-openjdk")
    java_link = destination / "usr/bin/java"
    java_link.parent.mkdir(parents=True, exist_ok=True)
    java_link.symlink_to("../lib/jvm/default-jvm/bin/java")

    profile = destination / "etc/profile.d/openjdk21.sh"
    profile.parent.mkdir(parents=True, exist_ok=True)
    profile.write_text(
        "export JAVA_HOME=/usr/lib/jvm/default-jvm\n"
        "export PATH=\"$JAVA_HOME/bin:$PATH\"\n",
        encoding="utf-8",
    )
    profile.chmod(0o644)

    smoke = destination / "opt/wateros/bin/wos-jvm-smoke"
    smoke.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(package / "scripts/wos-jvm-smoke", smoke)
    smoke.chmod(smoke.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    network_smoke = destination / "opt/wateros/bin/wos-jvm-network"
    shutil.copy2(package / "scripts/wos-jvm-network", network_smoke)
    network_smoke.chmod(
        network_smoke.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
    )
    application_smoke = destination / "opt/wateros/bin/wos-jvm-application"
    shutil.copy2(package / "scripts/wos-jvm-application", application_smoke)
    application_smoke.chmod(
        application_smoke.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
    )
    strict_smoke = destination / "opt/wateros/bin/wos-jvm-strict"
    shutil.copy2(package / "scripts/wos-jvm-strict", strict_smoke)
    strict_smoke.chmod(
        strict_smoke.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
    )
    install_test_classes(package, destination)
    install_java_truststore(package, destination)
    cacerts_link = jvm_root / "java-21-openjdk/lib/security/cacerts"
    if cacerts_link.exists() or cacerts_link.is_symlink():
        cacerts_link.unlink()
    cacerts_link.symlink_to("/etc/ssl/certs/java/cacerts")

    java = jvm_root / "java-21-openjdk/bin/java"
    validate_elf(java, context["readelf"], context["arch"])
    if not (jvm_root / "java-21-openjdk/lib/server/libjvm.so").is_file():
        raise RuntimeError("OpenJDK package is missing HotSpot libjvm.so")
    for runtime_file in ("lib/jspawnhelper", "lib/libnio.so"):
        if not (jvm_root / "java-21-openjdk" / runtime_file).is_file():
            raise RuntimeError(f"OpenJDK package is missing {runtime_file}")
    if context["arch"] == "rv":
        loader = destination / "lib/ld-musl-riscv64.so.1"
    else:
        loader = destination / "lib64/ld-linux-loongarch-lp64d.so.1"
    if not loader.exists():
        raise RuntimeError("OpenJDK package is missing the architecture dynamic loader")
    if not (destination / "usr/lib/libz.so.1").exists():
        raise RuntimeError("OpenJDK package is missing zlib")
    if (not cacerts_link.is_symlink()
            or os.readlink(cacerts_link) != "/etc/ssl/certs/java/cacerts"
            or not (destination / "etc/ssl/certs/java/cacerts").is_file()):
        raise RuntimeError("OpenJDK default Java truststore is unavailable")
    if not (destination / "etc/ssl/certs/ca-certificates.crt").is_file():
        raise RuntimeError("system CA certificate bundle is unavailable")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
