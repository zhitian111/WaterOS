#!/usr/bin/env python3
"""配置脚本共用的 Cargo manifest 与 feature 图解析工具。"""
from __future__ import annotations

import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Set, Tuple


@dataclass(frozen=True)
class Dep:
    dep_name: str  # dependency key in [dependencies]
    package: str  # actual package name (Cargo.toml package/name)
    path: Optional[str]  # local path if any (as written)


@dataclass
class Crate:
    package: str
    manifest_path: str
    deps: List[Dep]
    features: Dict[str, List[str]]


_STRING_RE = re.compile(r"""(["'])(.*?)\1""")
_IDENT_RE = re.compile(r"^[A-Za-z0-9_-]+$")


def _strip_comment(line: str) -> str:
    # Cargo.toml 的字符串基本不会包含 '#'(注释)；这里做简化处理
    if "#" in line:
        return line.split("#", 1)[0]
    return line


def _parse_array_of_strings(block: str) -> List[str]:
    # 从形如 [ "a", 'b', c ] 的 block 提取字符串字面量
    return [m.group(2).strip() for m in _STRING_RE.finditer(block)]


def _parse_cargo_toml_simple(manifest_path: Path) -> Dict[str, Any]:
    """
    简易解析 Cargo.toml（仅覆盖本项目用到的部分）：
    - [package].name
    - [dependencies] 中 { package = "...", path = "..." } 字段
    - [features] 中 key = [ ... ]（支持多行数组）
    目标：兼容 Python 3.8+，不依赖 tomllib/toml 第三方包。
    """
    result: Dict[str, Any] = {"package": {}, "dependencies": {}, "features": {}}
    text_lines = manifest_path.read_text(encoding="utf-8").splitlines()

    section: Optional[str] = None
    i = 0
    while i < len(text_lines):
        raw = text_lines[i]
        line = _strip_comment(raw).strip()
        i += 1

        if not line:
            continue
        m = re.match(r"^\[(?P<section>[A-Za-z0-9_-]+)\]\s*$", line)
        if m:
            section = m.group("section").strip()
            continue

        if section == "package":
            m = re.match(r"^name\s*=\s*(?P<q>['\"])(?P<name>.*?)\1\s*$", line)
            if m:
                result["package"]["name"] = m.group("name").strip()
            continue

        if section == "dependencies":
            # 处理单行或多行的 { ... } dict
            m = re.match(r"^(?P<key>[A-Za-z0-9_-]+)\s*=\s*\{(?P<rest>.*)$", line)
            if not m:
                continue
            dep_name = m.group("key")
            rest = m.group("rest").strip()
            # 收集到匹配到 '}' 结束
            while "}" not in rest and i < len(text_lines):
                rest = rest + " " + _strip_comment(text_lines[i]).strip()
                i += 1
            # 截断到 '}'
            if "}" in rest:
                rest = rest.split("}", 1)[0]
            # 解析 package/path
            pkg = None
            path = None
            mpkg = re.search(r'package\s*=\s*(?P<q>["\'])(?P<v>.*?)(?P=q)', rest)
            mpath = re.search(r'path\s*=\s*(?P<q>["\'])(?P<v>.*?)(?P=q)', rest)
            if mpkg:
                pkg = mpkg.group("v").strip()
            if mpath:
                path = mpath.group("v").strip()
            if pkg:
                result["dependencies"][dep_name] = {"package": pkg, "path": path}
            else:
                result["dependencies"][dep_name] = {"package": dep_name, "path": path}
            continue

        if section == "features":
            # key = [ ... ]（支持多行数组）
            m = re.match(r"^(?P<key>[A-Za-z0-9_-]+)\s*=\s*\[(?P<after>.*)$", line)
            if not m:
                # 支持 features value 用多行时的 key 行没写 '[' 的情况（当前仓库基本不需要）
                continue
            feat_key = m.group("key")
            block = m.group("after")
            while "]" not in block and i < len(text_lines):
                block = block + "\n" + _strip_comment(text_lines[i])
                i += 1
            # block 现在包含直到 ']' 前的内容
            if "]" in block:
                block = block.split("]", 1)[0]
            vals = _parse_array_of_strings(block)
            result["features"][feat_key] = vals
            continue

    return result


def _as_str_list(v: Any) -> List[str]:
    if v is None:
        return []
    if isinstance(v, list):
        out: List[str] = []
        for x in v:
            if isinstance(x, str):
                out.append(x)
        return out
    return []


def _parse_deps(tbl: Dict[str, Any]) -> List[Dep]:
    out: List[Dep] = []
    deps = tbl.get("dependencies", {})
    if not isinstance(deps, dict):
        return out
    for dep_name, spec in deps.items():
        if not isinstance(dep_name, str):
            continue
        package = dep_name
        path = None
        if isinstance(spec, str):
            # version string
            pass
        elif isinstance(spec, dict):
            pkg = spec.get("package")
            if isinstance(pkg, str) and pkg.strip():
                package = pkg.strip()
            p = spec.get("path")
            if isinstance(p, str) and p.strip():
                path = p.strip()
        out.append(Dep(dep_name=dep_name, package=package, path=path))
    return out


def _parse_features(tbl: Dict[str, Any]) -> Dict[str, List[str]]:
    feats = tbl.get("features", {})
    if not isinstance(feats, dict):
        return {}
    out: Dict[str, List[str]] = {}
    for k, v in feats.items():
        if isinstance(k, str):
            out[k] = _as_str_list(v)
    return out


def load_crate(manifest_path: Path) -> Optional[Crate]:
    try:
        tbl = _parse_cargo_toml_simple(manifest_path)
    except Exception:
        return None
    pkg = tbl.get("package", {})
    if not isinstance(pkg, dict):
        return None
    name = pkg.get("name")
    if not isinstance(name, str) or not name.strip():
        return None
    package = name.strip()
    deps = _parse_deps(tbl)
    features = _parse_features(tbl)
    return Crate(package=package, manifest_path=str(manifest_path), deps=deps, features=features)


def discover_manifests(root: Path) -> List[Path]:
    """
    递归扫描 root 下的 Cargo.toml，但会忽略：
    - target 目录
    - 我们自己创建的备份目录 .cargo-default-features-backup
    """
    manifests: List[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        # 跳过构建输出和备份目录
        for skip in ("target", ".cargo-default-features-backup"):
            if skip in dirnames:
                dirnames.remove(skip)
        if "Cargo.toml" in filenames:
            manifests.append(Path(dirpath) / "Cargo.toml")
    manifests.sort()
    return manifests


def build_index(manifests: Iterable[Path]) -> Tuple[Dict[str, Crate], Dict[str, Path]]:
    crates: Dict[str, Crate] = {}
    pkg_to_manifest: Dict[str, Path] = {}
    for m in manifests:
        c = load_crate(m)
        if c is None:
            continue
        # first wins; duplicate package names are unexpected
        if c.package not in crates:
            crates[c.package] = c
            pkg_to_manifest[c.package] = m
    return crates, pkg_to_manifest


def _indent(n: int) -> str:
    return "  " * n


_API_IMPL_RE = re.compile(r"(?:^|[-_])(api|impl)(?:$|[-_])", re.IGNORECASE)


def is_api_or_impl_package(package: str) -> bool:
    """
    用户约定：package 名字里带 api / impl 的，都视作 api/impl crate。
    这里按 token 边界匹配（-/_ 分隔）以避免误伤。
    """
    return _API_IMPL_RE.search(package) is not None


def _is_local_dep(dep: Dep, base_manifest: Path, pkg_to_manifest: Dict[str, Path]) -> bool:
    if dep.path:
        # if path resolves to an existing manifest, treat as local
        p = (base_manifest.parent / dep.path).resolve()
        if (p / "Cargo.toml").exists():
            return True
        if p.exists() and p.is_file() and p.name == "Cargo.toml":
            return True
    return dep.package in pkg_to_manifest


def _resolve_local_dep_manifest(dep: Dep, base_manifest: Path, pkg_to_manifest: Dict[str, Path]) -> Optional[Path]:
    if dep.path:
        p = (base_manifest.parent / dep.path).resolve()
        if p.is_file() and p.name == "Cargo.toml":
            return p
        if (p / "Cargo.toml").exists():
            return (p / "Cargo.toml").resolve()
    return pkg_to_manifest.get(dep.package)


def collect_cli_feature_candidates(crate: Crate) -> Tuple[List[str], Set[str]]:
    """
    Returns (feature_keys, referenced_dep_features).
    feature_keys includes all keys in [features] except 'default'.
    referenced_dep_features includes tokens like 'dep/feat' that appear in any feature value list,
    as well as plain 'dep' entries (optional dep feature) if present.
    """
    keys = [k for k in crate.features.keys() if k != "default"]
    keys.sort()
    referenced: Set[str] = set()
    for k, vals in crate.features.items():
        for t in vals:
            if not isinstance(t, str):
                continue
            s = t.strip()
            if not s or s.startswith("dep:"):
                # dep:foo is for optional dependency enabling; not a CLI feature token itself
                continue
            referenced.add(s)
    return keys, referenced


def default_enabled(crate: Crate) -> Set[str]:
    defaults = set(_as_str_list(crate.features.get("default")))
    return set(s.strip() for s in defaults if isinstance(s, str) and s.strip())


def feature_tree_lines(
    start_pkg: str,
    crates: Dict[str, Crate],
    pkg_to_manifest: Dict[str, Path],
    include_all_packages: bool,
) -> List[str]:
    lines: List[str] = []
    pkgs = sorted(crates.keys()) if include_all_packages else [start_pkg]
    for pkg in pkgs:
        crate = crates.get(pkg)
        if crate is None:
            continue
        lines.extend(_crate_feature_tree(crate, crates, pkg_to_manifest))
        lines.append("")
    return lines


def local_dep_packages(crate: Crate, crates: Dict[str, Crate], pkg_to_manifest: Dict[str, Path]) -> List[str]:
    base_manifest = Path(crate.manifest_path)
    out: List[str] = []
    for dep in crate.deps:
        if not _is_local_dep(dep, base_manifest, pkg_to_manifest):
            continue
        if dep.package in crates:
            out.append(dep.package)
    # stable
    out = sorted(set(out))
    return out


def selectable_api_impl_features(crate: Crate) -> List[str]:
    """
    只导出 api/impl 选择项：feature key 里包含 api 或 impl 的（如 api-v0 / impl-dummy）。
    """
    keys = [k for k in crate.features.keys() if k != "default"]
    out: List[str] = []
    for k in keys:
        if _API_IMPL_RE.search(k) is not None:
            out.append(k)
    out.sort()
    return out


def parse_config_tree_enabled(config_path: Path) -> Dict[str, Set[str]]:
    """
    解析 config.conf（缩进树格式），返回每个 crate 启用的 feature 集合（只收集未注释行）。
    注意：这里不做“api/impl 过滤”，由生成侧保证；解析侧保持通用。
    """
    enabled: Dict[str, Set[str]] = {}
    if not config_path.exists():
        return enabled

    def indent_of(s: str) -> int:
        return len(s) - len(s.lstrip(" "))

    stack: List[Tuple[int, str]] = []  # (indent, crate)
    with config_path.open("r", encoding="utf-8") as f:
        for raw in f:
            if not raw.strip():
                continue
            if raw.lstrip().startswith("#"):
                continue
            ind = indent_of(raw.rstrip("\n"))
            line = raw.strip()
            if line.endswith(":") and " " not in line[:-1]:
                crate = line[:-1]
                while stack and stack[-1][0] >= ind:
                    stack.pop()
                stack.append((ind, crate))
                enabled.setdefault(crate, set())
                continue
            if not stack:
                continue
            # feature line belongs to current crate (top of stack)
            crate = stack[-1][1]
            enabled.setdefault(crate, set()).add(line)
    return enabled


def parse_config_tree_enabled_at_indent(config_path: Path, header_indent: int = 0) -> Dict[str, Set[str]]:
    """
    只提取指定缩进层级的 crate（例如 header_indent=0 表示顶层 crate），并返回它们的启用 feature。
    """
    enabled: Dict[str, Set[str]] = {}
    if not config_path.exists():
        return enabled

    def indent_of(s: str) -> int:
        return len(s) - len(s.lstrip(" "))

    current: Optional[str] = None
    current_indent: Optional[int] = None

    with config_path.open("r", encoding="utf-8") as f:
        for raw in f:
            if not raw.strip():
                continue
            if raw.lstrip().startswith("#"):
                continue
            ind = indent_of(raw.rstrip("\n"))
            line = raw.strip()
            if line.endswith(":") and " " not in line[:-1]:
                name = line[:-1]
                current = name if ind == header_indent else None
                current_indent = ind
                if current is not None:
                    enabled.setdefault(current, set())
                continue
            if current is None:
                continue
            # collect only direct children of this header indent (+2 spaces convention, but allow any >)
            enabled.setdefault(current, set()).add(line)

    return enabled


def dep_name_for_package(parent: Crate, child_package: str) -> Optional[str]:
    """
    在 parent 的依赖中，找到指向 child_package 的 dependency name。
    """
    for d in parent.deps:
        if d.package == child_package:
            return d.dep_name
    return None


def bubble_features_to_root(
    root_package: str,
    enabled_by_pkg: Dict[str, Set[str]],
    crates: Dict[str, Crate],
) -> Tuple[Set[str], List[str]]:
    """
    将 config.conf 中各 crate 的选择项“向上回溯”到 root_package 的直接依赖 feature flag。

    返回 (root_feature_flags, warnings)
    - root_feature_flags: 适合传给 `cargo build -p <root_package> --features "<comma list>"` 的 token 集合，
      token 形如 "dep/feat"（只一层 slash，依赖必须是 root 的直接 dependency）。
    - warnings: 无法回溯/无法匹配时的警告信息。
    """
    warnings: List[str] = []
    root_flags: Set[str] = set()

    root = crates.get(root_package)
    if root is None:
        return set(), [f"root package '{root_package}' not found"]

    # root direct deps: dep_name -> child package
    direct: Dict[str, str] = {d.dep_name: d.package for d in root.deps}
    direct_pkg_to_dep: Dict[str, str] = {pkg: dep for dep, pkg in direct.items()}

    # Build reverse edges: child_package -> list[parent_package]
    parents_of: Dict[str, List[str]] = {}
    for p_pkg, p in crates.items():
        for d in p.deps:
            parents_of.setdefault(d.package, []).append(p_pkg)

    # Worklist: desired (pkg, feat)
    work: List[Tuple[str, str]] = []
    for pkg, feats in enabled_by_pkg.items():
        for feat in feats:
            work.append((pkg, feat))

    seen: Set[Tuple[str, str]] = set()

    while work:
        pkg, feat = work.pop()
        if (pkg, feat) in seen:
            continue
        seen.add((pkg, feat))

        # root crate: features are passed directly to cargo --features
        if pkg == root_package:
            root_flags.add(feat)
            continue

        # If pkg is a direct dep of root, we're done: enable dep/feat at root.
        if pkg in direct_pkg_to_dep:
            dep = direct_pkg_to_dep[pkg]
            root_flags.add(f"{dep}/{feat}")
            continue

        # Transitive api-* features are shared by multiple impl features. Bubbling
        # them upward through parent features can accidentally select every arch
        # impl that also enables the same API (for example RISC-V + LoongArch).
        if feat.startswith("api-"):
            continue

        # Otherwise, try to bubble to one of its parents by selecting a parent feature
        # that enables "<depName>/<feat>".
        p_candidates = parents_of.get(pkg, [])
        if not p_candidates:
            warnings.append(f"无法回溯: {pkg}:{feat}（没有上游依赖它的本地 crate）")
            continue

        bubbled = False
        for p_pkg in p_candidates:
            parent = crates.get(p_pkg)
            child = crates.get(pkg)
            if parent is None or child is None:
                continue
            dep_name = dep_name_for_package(parent, pkg)
            if not dep_name:
                continue
            needle = f"{dep_name}/{feat}"
            # find parent feature(s) referencing needle
            for p_feat, vals in parent.features.items():
                if p_feat == "default":
                    continue
                if needle in vals:
                    work.append((p_pkg, p_feat))
                    bubbled = True
        if not bubbled:
            warnings.append(f"无法回溯: {pkg}:{feat}（找不到上游 feature 引用它）")

    return root_flags, warnings


def _crate_feature_tree(crate: Crate, crates: Dict[str, Crate], pkg_to_manifest: Dict[str, Path]) -> List[str]:
    base_manifest = Path(crate.manifest_path)
    defaults = default_enabled(crate)
    lines: List[str] = []
    lines.append(f"{crate.package}  ({os.path.relpath(crate.manifest_path)})")
    if not crate.features:
        lines.append(_indent(1) + "(no [features])")
        return lines

    # stable order: default first, then others
    keys = list(crate.features.keys())
    keys.sort(key=lambda k: (0 if k == "default" else 1, k))
    for feat in keys:
        vals = crate.features.get(feat, [])
        mark = " [default]" if feat in defaults or feat == "default" else ""
        lines.append(_indent(1) + f"- {feat}{mark}")
        if not vals:
            continue
        for t in vals:
            lines.append(_indent(2) + f"- {t}")
            # if it's a local dependency feature dep/feat, print nested package block
            if "/" in t:
                dep_name = t.split("/", 1)[0].strip()
                dep = next((d for d in crate.deps if d.dep_name == dep_name), None)
                if dep and _is_local_dep(dep, base_manifest, pkg_to_manifest):
                    dep_m = _resolve_local_dep_manifest(dep, base_manifest, pkg_to_manifest)
                    if dep_m and dep.package in crates:
                        dep_crate = crates[dep.package]
                        lines.append(_indent(3) + f"> {dep_crate.package}")
                        # one-level features listing (avoid huge recursion)
                        dep_defaults = default_enabled(dep_crate)
                        dep_keys = list(dep_crate.features.keys())
                        dep_keys.sort(key=lambda k: (0 if k == "default" else 1, k))
                        for df in dep_keys:
                            dmark = " [default]" if df in dep_defaults or df == "default" else ""
                            lines.append(_indent(4) + f"- {df}{dmark}")
    return lines


def print_json_summary(root: Path) -> Dict[str, Any]:
    manifests = discover_manifests(root)
    crates, _ = build_index(manifests)
    return {"root": str(root), "manifests": [str(m) for m in manifests], "packages": sorted(crates.keys())}


def main(argv: List[str]) -> int:
    # This module is intended to be invoked by bash scripts.
    # Minimal CLI is provided for internal piping/debugging.
    if len(argv) >= 2 and argv[1] == "--summary":
        root = Path(argv[2]) if len(argv) >= 3 else Path(".")
        data = print_json_summary(root)
        import json

        print(json.dumps(data, ensure_ascii=False, indent=2))
        return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
