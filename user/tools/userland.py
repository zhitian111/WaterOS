#!/usr/bin/env python3
"""Build composable WaterOS userspace packages and EXT4 images.

The tool deliberately uses only the Python standard library.  Package build
scripts receive a JSON context and install into an isolated DESTDIR; only this
orchestrator is allowed to merge package outputs into a root filesystem.
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import Iterable

USER_ROOT = Path(__file__).resolve().parents[1]
CONFIG_ROOT = USER_ROOT / "configs"
PACKAGE_ROOT = USER_ROOT / "packages"
BUILD_ROOT = USER_ROOT / "build"
SOURCE_DATE_EPOCH = "1704067200"


class UserlandError(RuntimeError):
    """A deterministic configuration, build or image composition error."""


@dataclasses.dataclass(frozen=True)
class Architecture:
    name: str
    triple: str
    cross_compile: str
    kernel_arch: str
    cflags: tuple[str, ...]
    elf_machine: str


@dataclasses.dataclass(frozen=True)
class Package:
    name: str
    version: str
    directory: Path
    source: Path | None
    architectures: tuple[str, ...]
    dependencies: tuple[str, ...]
    build_script: Path
    install_prefix: str
    allow_overwrite: tuple[str, ...]
    inputs: tuple[Path, ...]


OVERLAY_REPLACE_PREFIXES = (
    "/bin", "/sbin", "/usr/bin", "/usr/sbin", "/etc/wateros",
    "/opt/wateros", "/root", "/var/lib/wateros",
)


def load_toml(path: Path) -> dict:
    try:
        with path.open("rb") as source:
            return tomllib.load(source)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise UserlandError(f"cannot read {path}: {error}") from error


def _is_arch_linux() -> bool:
    try:
        return "ID=arch" in Path("/etc/os-release").read_text(encoding="utf-8")
    except OSError:
        return False


def _archlinux_rv_musl_prefix() -> str | None:
    """Return a complete RV musl prefix for Arch's split toolchain packages."""
    if not _is_arch_linux():
        return None
    tools = {
        "gcc": shutil.which("riscv64-linux-musl-gcc"),
        "ar": shutil.which("riscv64-linux-gnu-ar"),
        "strip": shutil.which("riscv64-linux-gnu-strip"),
        "readelf": shutil.which("riscv64-linux-gnu-readelf"),
    }
    if not all(tools.values()):
        return None
    directory = BUILD_ROOT / "toolchains" / "rv" / "archlinux-compat" / "bin"
    directory.mkdir(parents=True, exist_ok=True)
    prefix = directory / "riscv64-linux-musl-"
    for suffix, target in tools.items():
        link = Path(f"{prefix}{suffix}")
        if link.is_symlink() and os.readlink(link) == target:
            continue
        if link.exists() or link.is_symlink():
            raise UserlandError(f"Arch RV compatibility tool is not a symlink: {link}")
        link.symlink_to(target)
    return str(prefix)


def load_architecture(name: str) -> Architecture:
    raw = load_toml(CONFIG_ROOT / "architectures.toml").get("architectures", {})
    if name not in raw:
        raise UserlandError(f"unknown architecture {name!r}; choose one of {sorted(raw)}")
    entry = raw[name]
    cross = os.environ.get(entry["cross_env"])
    if cross is None:
        managed_prefix = (BUILD_ROOT / "toolchains" / name / "bin"
                          / entry["cross_compile"])
        managed_compiler = Path(f"{managed_prefix}gcc")
        compat_prefix = _archlinux_rv_musl_prefix() if name == "rv" else None
        cross = (str(managed_prefix) if managed_compiler.is_file()
                 else compat_prefix or entry["cross_compile"])
    if not cross:
        raise UserlandError(f"empty cross compiler prefix for architecture {name}")
    return Architecture(name=name, triple=entry["triple"], cross_compile=cross,
                        kernel_arch=entry["kernel_arch"],
                        cflags=tuple(entry.get("cflags", ())),
                        elf_machine=entry["elf_machine"])


def _safe_relative(path: str, *, field: str) -> Path:
    candidate = Path(path)
    if candidate.is_absolute() or ".." in candidate.parts:
        raise UserlandError(f"{field} must stay inside user/: {path!r}")
    return candidate


def load_package(name: str) -> Package:
    directory = PACKAGE_ROOT / name
    raw = load_toml(directory / "package.toml").get("package", {})
    if raw.get("name") != name:
        raise UserlandError(f"package directory {name!r} has mismatched package.name")
    source_text = raw.get("source", "")
    source = USER_ROOT / _safe_relative(source_text, field="package.source") if source_text else None
    build_script = directory / _safe_relative(raw.get("build", "build.py"), field="package.build")
    inputs = tuple(USER_ROOT / _safe_relative(item, field="package.inputs")
                   for item in raw.get("inputs", ()))
    package = Package(name=name, version=str(raw.get("version", "0")), directory=directory,
                      source=source,
                      architectures=tuple(raw.get("architectures", ())),
                      dependencies=tuple(raw.get("dependencies", ())),
                      build_script=build_script,
                      install_prefix=str(raw.get("install_prefix", "/")),
                      allow_overwrite=tuple(raw.get("allow_overwrite", ())),
                      inputs=inputs)
    if not package.build_script.is_file():
        raise UserlandError(f"package {name} build script is missing: {package.build_script}")
    if package.source is not None and not package.source.is_dir():
        raise UserlandError(f"package {name} source is missing: {package.source}")
    return package


def available_package_names(architecture: str) -> tuple[str, ...]:
    """返回当前架构支持的全部顶层 package，供默认的 `all` 选择使用。"""
    result: list[str] = []
    for metadata in sorted(PACKAGE_ROOT.glob("*/package.toml")):
        name = metadata.parent.name
        # 这里只读取选择所需的最小元数据。这样用户仍可通过 --exclude-packages
        # 排除源码尚未准备好的可选 package，而不会在排除生效前就验证其源码。
        raw = load_toml(metadata).get("package", {})
        if raw.get("name") != name:
            raise UserlandError(f"package directory {name!r} has mismatched package.name")
        if architecture in raw.get("architectures", ()):
            result.append(name)
    if not result:
        raise UserlandError(f"no packages support architecture {architecture}")
    return tuple(result)


def parse_package_names(value: str, architecture: str) -> tuple[str, ...]:
    """解析 `all` 或逗号/空白分隔的自定义 package 列表。"""
    normalized = value.replace(",", " ").split()
    if not normalized:
        raise UserlandError("package selection must not be empty")
    if "all" in normalized:
        if len(normalized) != 1:
            raise UserlandError("'all' cannot be combined with explicit package names")
        return available_package_names(architecture)
    return tuple(dict.fromkeys(normalized))


def parse_excluded_package_names(value: str) -> tuple[str, ...]:
    """解析逗号或空白分隔的排除列表；空值表示不排除。"""
    return tuple(dict.fromkeys(value.replace(",", " ").split()))


def exclude_packages(package_names: tuple[str, ...], excluded_names: tuple[str, ...],
                     architecture: str) -> tuple[tuple[str, ...], dict[str, tuple[str, ...]]]:
    """排除指定 package，并级联排除依赖它们的顶层 package。

    返回最终顶层列表和 ``被跳过 package -> 根排除项``。依赖闭包仍由
    :func:`resolve_packages` 统一展开，因此这里不会改变正常的拓扑顺序。
    """
    if not excluded_names:
        return package_names, {}
    excluded = set(excluded_names)
    memo: dict[str, frozenset[str]] = {}
    visiting: list[str] = []

    def blockers(name: str) -> frozenset[str]:
        if name in memo:
            return memo[name]
        if name in excluded:
            result = frozenset((name,))
            memo[name] = result
            return result
        if name in visiting:
            cycle = " -> ".join([*visiting, name])
            raise UserlandError(f"package dependency cycle: {cycle}")
        visiting.append(name)
        package = load_package(name)
        if architecture not in package.architectures:
            raise UserlandError(f"package {name} does not support {architecture}")
        result = frozenset().union(*(blockers(dependency)
                                     for dependency in package.dependencies))
        visiting.pop()
        memo[name] = result
        return result

    kept: list[str] = []
    skipped: dict[str, tuple[str, ...]] = {}
    for name in package_names:
        reasons = blockers(name)
        if reasons:
            skipped[name] = tuple(sorted(reasons))
        else:
            kept.append(name)
    if not kept:
        raise UserlandError("package exclusions removed every selected package")
    return tuple(kept), skipped


def resolve_packages(package_names: Iterable[str], architecture: str) -> list[Package]:
    result: list[Package] = []
    visiting: list[str] = []
    complete: set[str] = set()

    def visit(name: str) -> None:
        if name in complete:
            return
        if name in visiting:
            cycle = " -> ".join([*visiting, name])
            raise UserlandError(f"package dependency cycle: {cycle}")
        visiting.append(name)
        package = load_package(name)
        if architecture not in package.architectures:
            raise UserlandError(f"package {name} does not support {architecture}")
        for dependency in package.dependencies:
            visit(dependency)
        visiting.pop()
        complete.add(name)
        result.append(package)

    for package_name in package_names:
        visit(package_name)
    return result


def _cache_entry_is_ignored(path: Path) -> bool:
    """Exclude interpreter/VCS artifacts which are not package inputs."""
    return ("__pycache__" in path.parts or ".git" in path.parts
            or path.suffix in (".pyc", ".pyo"))


def hash_path(path: Path, digest: "hashlib._Hash") -> None:
    if not path.exists() and not path.is_symlink():
        raise UserlandError(f"cache input is missing: {path}")
    if path.is_symlink():
        digest.update(f"{path.lstat().st_mode & 0o7777:o}\0".encode())
        digest.update(b"L\0" + os.readlink(path).encode() + b"\0")
        return
    if path.is_file():
        digest.update(b"F\0" + path.name.encode() + b"\0")
        digest.update(f"{path.stat().st_mode & 0o7777:o}\0".encode())
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
        return
    digest.update(b"D\0" + f"{path.stat().st_mode & 0o7777:o}\0".encode())
    for entry in sorted(path.rglob("*"), key=lambda item: item.as_posix()):
        if _cache_entry_is_ignored(entry.relative_to(path)):
            continue
        relative = entry.relative_to(path).as_posix().encode()
        digest.update(relative + b"\0")
        if entry.is_symlink():
            digest.update(f"{entry.lstat().st_mode & 0o7777:o}\0".encode())
            digest.update(b"L\0" + os.readlink(entry).encode() + b"\0")
        elif entry.is_file():
            digest.update(b"F\0")
            digest.update(f"{entry.stat().st_mode & 0o7777:o}\0".encode())
            with entry.open("rb") as source:
                for chunk in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(chunk)
        elif entry.is_dir():
            digest.update(b"D\0" + f"{entry.stat().st_mode & 0o7777:o}\0".encode())


def package_cache_key(package: Package, architecture: Architecture,
                      toolchain_version: str) -> str:
    digest = hashlib.sha256()
    digest.update(b"wateros-userland-package-v1\0")
    digest.update(json.dumps(dataclasses.asdict(architecture), sort_keys=True).encode())
    digest.update(toolchain_version.encode())
    hash_path(package.directory, digest)
    if package.source is not None:
        hash_path(package.source, digest)
    for extra in package.inputs:
        hash_path(extra, digest)
    return digest.hexdigest()


def find_tool(name: str) -> str:
    result = shutil.which(name)
    if result is None:
        raise UserlandError(f"required tool not found: {name}")
    return result


def find_readelf(architecture: Architecture) -> str:
    cross = shutil.which(f"{architecture.cross_compile}readelf")
    return cross or find_tool("readelf")


def compiler_version(architecture: Architecture) -> str:
    compiler = find_tool(f"{architecture.cross_compile}gcc")
    completed = subprocess.run([compiler, "--version"], check=True, text=True,
                               capture_output=True)
    return completed.stdout.splitlines()[0]


def doctor(architecture: Architecture, *, static_probe: bool = True) -> list[str]:
    errors: list[str] = []
    if sys.version_info < (3, 11):
        errors.append("Python 3.11 or newer is required")
    for tool in ("gcc", "make", "patch", "mke2fs", "debugfs", "e2fsck", "dumpe2fs"):
        if shutil.which(tool) is None:
            errors.append(f"missing host tool: {tool}")
    for suffix in ("gcc", "ar", "strip"):
        if shutil.which(f"{architecture.cross_compile}{suffix}") is None:
            errors.append(f"missing cross tool: {architecture.cross_compile}{suffix}")
    busybox_required = ("Makefile", "Config.in", "LICENSE")
    if not all((USER_ROOT / "vendor/busybox" / item).is_file()
               for item in busybox_required):
        errors.append("vendored BusyBox source is incomplete")
    compiler = shutil.which(f"{architecture.cross_compile}gcc")
    readelf = None
    try:
        readelf = find_readelf(architecture)
    except UserlandError as error:
        errors.append(str(error))
    if compiler:
        try:
            machine = subprocess.run([compiler, "-dumpmachine"], check=True, text=True,
                                     capture_output=True).stdout.strip()
            expected = "riscv64" if architecture.name == "rv" else "loongarch64"
            if expected not in machine:
                errors.append(f"compiler target {machine!r} is not {expected}")
        except subprocess.CalledProcessError as error:
            errors.append(f"cannot query compiler target: {error}")
    if static_probe and compiler and readelf:
        try:
            with tempfile.TemporaryDirectory(prefix="wateros-user-doctor-") as temporary:
                root = Path(temporary)
                source = root / "probe.c"
                binary = root / "probe"
                source.write_text("int main(void) { return 0; }\n", encoding="utf-8")
                subprocess.run([compiler, *architecture.cflags, "-static", str(source),
                                "-o", str(binary)], check=True, capture_output=True)
                header = subprocess.run([readelf, "-h", str(binary)], check=True,
                                        text=True, capture_output=True).stdout
                program = subprocess.run([readelf, "-l", str(binary)], check=True,
                                         text=True, capture_output=True).stdout
                dynamic = subprocess.run([readelf, "-d", str(binary)], check=True,
                                         text=True, capture_output=True).stdout
                if architecture.elf_machine.lower() not in header.lower():
                    errors.append("static probe has the wrong ELF architecture")
                if "INTERP" in program:
                    errors.append("static probe unexpectedly contains PT_INTERP")
                if "NEEDED" in dynamic:
                    errors.append("static probe unexpectedly needs a shared library")
        except subprocess.CalledProcessError as error:
            detail = error.stderr.decode(errors="replace") if isinstance(error.stderr, bytes) else error.stderr
            errors.append(f"static link probe failed: {(detail or error).strip()}")
    return errors


def _remove(path: Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.exists():
        shutil.rmtree(path)


def build_package(package: Package, architecture: Architecture, jobs: int,
                  toolchain_version: str) -> tuple[Path, str]:
    cache_key = package_cache_key(package, architecture, toolchain_version)
    cache = BUILD_ROOT / "packages" / architecture.name / package.name / cache_key
    destdir = cache / "root"
    complete = cache / ".complete"
    if complete.is_file() and destdir.is_dir():
        print(f"[userland] cache hit {package.name} {cache_key[:12]}")
        return destdir, cache_key
    _remove(cache)
    cache.mkdir(parents=True)
    destdir.mkdir()
    work = BUILD_ROOT / "work" / architecture.name / f"{package.name}-{cache_key[:12]}"
    _remove(work)
    if package.source is not None:
        shutil.copytree(package.source, work, symlinks=True)
    else:
        work.mkdir(parents=True)
    patches = package.directory / "patches"
    if patches.is_dir():
        for patch in sorted(patches.glob("*.patch")):
            subprocess.run([find_tool("patch"), "-p1", "-i", str(patch)],
                           cwd=work, check=True)
    context = {
        "arch": architecture.name,
        "triple": architecture.triple,
        "cross_compile": architecture.cross_compile,
        "kernel_arch": architecture.kernel_arch,
        "cflags": list(architecture.cflags),
        "elf_machine": architecture.elf_machine,
        "readelf": find_readelf(architecture),
        "jobs": jobs,
        "source_date_epoch": SOURCE_DATE_EPOCH,
        "user_root": str(USER_ROOT),
        "package_dir": str(package.directory),
        "install_prefix": package.install_prefix,
        "source_dir": str(package.source) if package.source else "",
        "work_dir": str(work),
        "destdir": str(destdir),
    }
    context_path = cache / "context.json"
    context_path.write_text(json.dumps(context, indent=2, sort_keys=True) + "\n",
                            encoding="utf-8")
    print(f"[userland] build {package.name} {cache_key[:12]}")
    try:
        subprocess.run([sys.executable, str(package.build_script), "--context",
                        str(context_path)], cwd=USER_ROOT, check=True)
    except Exception:
        _remove(cache)
        raise
    complete.write_text(cache_key + "\n", encoding="utf-8")
    return destdir, cache_key


def iter_entries(root: Path) -> Iterable[Path]:
    return sorted(root.rglob("*"), key=lambda path: (len(path.relative_to(root).parts),
                                                     path.relative_to(root).as_posix()))


def _lexists(path: Path) -> bool:
    return path.exists() or path.is_symlink()


def merge_package(source: Path, staging: Path, *, package: Package,
                  owners: dict[str, str]) -> None:
    allowed = set(package.allow_overwrite)
    for entry in iter_entries(source):
        relative = entry.relative_to(source)
        logical = "/" + relative.as_posix()
        target = staging / relative
        previous = owners.get(logical)
        if entry.is_dir() and not entry.is_symlink():
            if _lexists(target) and not target.is_dir():
                raise UserlandError(f"{package.name}: directory conflicts with {logical} from {previous}")
            target.mkdir(parents=True, exist_ok=True)
            shutil.copystat(entry, target, follow_symlinks=False)
            owners.setdefault(logical, package.name)
            continue
        if _lexists(target):
            if logical not in allowed:
                raise UserlandError(f"{package.name}: path {logical} is already owned by {previous}")
            _remove(target)
        target.parent.mkdir(parents=True, exist_ok=True)
        if entry.is_symlink():
            target.symlink_to(os.readlink(entry))
        else:
            shutil.copy2(entry, target)
        owners[logical] = package.name


def file_manifest(root: Path) -> list[dict[str, object]]:
    manifest: list[dict[str, object]] = []
    for entry in iter_entries(root):
        relative = "/" + entry.relative_to(root).as_posix()
        stat = entry.lstat()
        item: dict[str, object] = {"path": relative, "mode": stat.st_mode & 0o7777}
        if entry.is_symlink():
            item.update(kind="symlink", target=os.readlink(entry))
        elif entry.is_dir():
            item.update(kind="directory")
        else:
            digest = hashlib.sha256(entry.read_bytes()).hexdigest()
            item.update(kind="file", size=stat.st_size, sha256=digest)
        manifest.append(item)
    return manifest


def build_packages(architecture: Architecture, package_names: tuple[str, ...],
                   jobs: int) -> Path:
    errors = doctor(architecture)
    if errors:
        raise UserlandError("environment check failed:\n  - " + "\n  - ".join(errors))
    toolchain = compiler_version(architecture)
    packages = resolve_packages(package_names, architecture.name)
    outputs: list[tuple[Package, Path, str]] = []
    for package in packages:
        root, key = build_package(package, architecture, jobs, toolchain)
        outputs.append((package, root, key))
    staging = BUILD_ROOT / "staging" / architecture.name / "rootfs"
    _remove(staging)
    staging.mkdir(parents=True)
    owners: dict[str, str] = {}
    for package, root, _ in outputs:
        merge_package(root, staging, package=package, owners=owners)
    metadata_dir = staging / "var/lib/wateros"
    metadata_dir.mkdir(parents=True, exist_ok=True)
    package_metadata = {
        "schema": 2,
        "architecture": architecture.name,
        "triple": architecture.triple,
        "requested_packages": list(package_names),
        "toolchain": toolchain,
        "packages": [
            {
                "name": package.name,
                "version": package.version,
                "input_sha256": key,
                "output_sha256": hashlib.sha256(
                    json.dumps(file_manifest(root), sort_keys=True,
                               separators=(",", ":")).encode()
                ).hexdigest(),
            }
            for package, root, key in outputs
        ],
    }
    (metadata_dir / "packages.json").write_text(
        json.dumps(package_metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    manifest = file_manifest(staging)
    manifest_path = BUILD_ROOT / "manifests" / f"wateros-{architecture.name}.json"
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n",
                             encoding="utf-8")
    print(f"[userland] staging: {staging}")
    print(f"[userland] manifest: {manifest_path}")
    return staging


def default_jobs() -> int:
    return max(1, os.cpu_count() or 1)


def add_build_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--arch", choices=("rv", "la"), required=True)
    parser.add_argument("--packages", default="all",
                        help="all or a comma-separated package list")
    parser.add_argument("--exclude-packages", default="",
                        help="comma-separated packages to skip; dependents are skipped too")
    parser.add_argument("--jobs", type=int, default=default_jobs())


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    doctor_parser = commands.add_parser("doctor", help="check host and cross tools")
    doctor_parser.add_argument("--arch", choices=("rv", "la"), required=True)
    build_parser = commands.add_parser("build", help="build and merge a rootfs staging tree")
    add_build_arguments(build_parser)
    image_parser = commands.add_parser("image", help="build a standalone EXT4 image")
    add_build_arguments(image_parser)
    image_parser.add_argument("--image-size-mb", type=int, default=256)
    image_parser.add_argument("--block-size", type=int, default=4096)
    image_parser.add_argument("--inode-size", type=int, default=256)
    image_parser.add_argument("--output", type=Path)
    overlay_parser = commands.add_parser("overlay", help="copy and augment an EXT4 image")
    add_build_arguments(overlay_parser)
    overlay_parser.add_argument("--base-image", type=Path, required=True)
    overlay_parser.add_argument("--output", type=Path)
    inspect_parser = commands.add_parser("inspect", help="inspect an image and embedded metadata")
    add_build_arguments(inspect_parser)
    inspect_parser.add_argument("--image", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_arguments()
    try:
        architecture = load_architecture(args.arch)
        if args.command == "doctor":
            errors = doctor(architecture)
            if errors:
                print("[userland] doctor failed:", file=sys.stderr)
                for error in errors:
                    print(f"  - {error}", file=sys.stderr)
                return 1
            print(f"[userland] doctor passed: arch={architecture.name} "
                  f"cross={architecture.cross_compile}")
            return 0
        package_names = parse_package_names(args.packages, architecture.name)
        excluded_names = parse_excluded_package_names(args.exclude_packages)
        package_names, skipped = exclude_packages(package_names, excluded_names,
                                                   architecture.name)
        for name, reasons in skipped.items():
            print(f"[userland] skip {name}: excluded dependency "
                  f"{','.join(reasons)}")
        if args.jobs < 1:
            raise UserlandError("--jobs must be positive")
        if args.command == "build":
            build_packages(architecture, package_names, args.jobs)
            return 0
        # Local import keeps package/config unit tests independent from e2fsprogs.
        import image as image_tool
        if args.command == "image":
            staging = build_packages(architecture, package_names, args.jobs)
            output = args.output or image_tool.default_image_path(architecture.name)
            image_tool.create_image(staging, output, architecture.name,
                                    args.image_size_mb, args.block_size, args.inode_size)
            return 0
        if args.command == "overlay":
            staging = build_packages(architecture, package_names, args.jobs)
            output = args.output or image_tool.default_overlay_path(args.base_image,
                                                                    architecture.name)
            image_tool.create_overlay(staging, args.base_image, output,
                                      OVERLAY_REPLACE_PREFIXES,
                                      architecture.name)
            return 0
        if args.command == "inspect":
            image_path = args.image or image_tool.default_image_path(architecture.name)
            image_tool.inspect_image(image_path)
            return 0
        raise UserlandError(f"unhandled command {args.command}")
    except (UserlandError, subprocess.CalledProcessError, OSError) as error:
        print(f"[userland] error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
