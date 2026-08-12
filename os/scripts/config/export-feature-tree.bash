#!/bin/sh
# 扫描所有本地 Cargo 清单，输出完整 feature 树以及当前默认启用的配置。
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=/dev/null
. "${SCRIPT_DIR}/../source/console.bash"

ROOT_DIR_DEFAULT=$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)
ROOT_DIR="${1:-$ROOT_DIR_DEFAULT}"

OUT_DIR="${ROOT_DIR}"
TREE_OUT="${OUT_DIR}/feature-tree.txt"
CONF_OUT="${OUT_DIR}/config.conf"

info "扫描 Cargo.toml: ${ROOT_DIR}"

PYTHONPATH="${SCRIPT_DIR}/.." python3 - <<'PY' "${ROOT_DIR}" "${TREE_OUT}" "${CONF_OUT}"
from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Dict, List, Set, Tuple

from source.feature_tools import (
    build_index,
    default_enabled,
    discover_manifests,
    feature_tree_lines,
    is_api_or_impl_package,
    local_dep_packages,
    selectable_api_impl_features,
)


def write_text(path: Path, lines: List[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def main() -> int:
    root = Path(sys.argv[1]).resolve()
    tree_out = Path(sys.argv[2]).resolve()
    conf_out = Path(sys.argv[3]).resolve()

    manifests = discover_manifests(root)
    crates, pkg_to_manifest = build_index(manifests)
    kernel_pkg = "wateros"
    if kernel_pkg not in crates:
        raise SystemExit("ERROR: 未找到根 crate 'wateros'，请检查 os/Cargo.toml")

    # 1) feature 树（按 package 分组）
    tree_lines = []
    tree_lines.append("# WaterOS feature tree (按 package 分组)")
    tree_lines.append(f"# root: {root}")
    tree_lines.append("")
    tree_lines.extend(feature_tree_lines(start_pkg=kernel_pkg, crates=crates, pkg_to_manifest=pkg_to_manifest, include_all_packages=True))
    write_text(tree_out, tree_lines)

    # 2) 单一总配置文件：只导出“非 api/impl crate”，并用缩进体现依赖树
    conf_lines: List[str] = []
    conf_lines.append("# WaterOS config.conf (只导出非 api/impl crate)")
    conf_lines.append(f"# root: {root}")
    conf_lines.append("# 规则：未注释行 = 启用；以 # 开头 = 禁用。")
    conf_lines.append("# 结构：crate 行以 ':' 结尾；其下缩进两格的是 feature。")
    conf_lines.append("# 组件 crate 只列出 api-* / impl-* 选择项；kernel crate 会列出全部 feature。")
    conf_lines.append("# 驱动类允许同时启用多个 impl。")
    conf_lines.append("")

    def add_crate(pkg: str, depth: int, visiting: Set[str]) -> None:
        if pkg in visiting:
            conf_lines.append("  " * depth + f"{pkg}:")
            conf_lines.append("  " * (depth + 1) + "# (cycle detected)")
            return
        c = crates.get(pkg)
        if c is None:
            return
        if is_api_or_impl_package(pkg):
            return
        visiting.add(pkg)

        conf_lines.append("  " * depth + f"{pkg}:")
        conf_lines.append("  " * (depth + 1) + f"# manifest: {os.path.relpath(c.manifest_path, start=str(root))}")

        defaults = default_enabled(c)
        if kernel_pkg is not None and pkg == kernel_pkg:
            # kernel crate 的 feature 都是“可直接配置”的
            choices = [k for k in c.features.keys() if k != "default"]
            choices.sort()
        else:
            # 组件 crate：只导出 api-* / impl-* 选择项
            choices = selectable_api_impl_features(c)
        if not choices:
            conf_lines.append("  " * (depth + 1) + "# (no api/impl selectable features)")
        else:
            for f in choices:
                if f in defaults:
                    conf_lines.append("  " * (depth + 1) + f)
                else:
                    conf_lines.append("  " * (depth + 1) + f"# {f}")

        # 递归依赖：只沿本地 crate 走（并且只显示非 api/impl crate）
        for dpkg in local_dep_packages(c, crates, pkg_to_manifest):
            add_crate(dpkg, depth + 1, visiting)

        visiting.remove(pkg)

    # 从 kernel_pkg 向下递归，输出“缩进树”
    add_crate(kernel_pkg, 0, set())
    conf_lines.append("")

    write_text(conf_out, conf_lines)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
PY

info "feature 树已生成: ${TREE_OUT}"
info "features 总配置已生成: ${CONF_OUT}"
