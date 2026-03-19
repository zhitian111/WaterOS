#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/source/console.bash"

if [[ "${WATEROS_SCRIPTS_QUIET:-0}" == "1" ]]; then
  trace() { :; }
  info() { :; }
  warning() { :; }
  debug() { :; }
fi

ROOT_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
ROOT_DIR="${ROOT_DIR_DEFAULT}"

CONF_PATH="${1:-${ROOT_DIR}/config.conf}"
ROOT_PKG="${2:-}"

info "从 config.conf 生成顶层 flags: root=${ROOT_PKG:-<auto>}"

if [[ ! -f "${CONF_PATH}" ]]; then
  warning "配置文件不存在: ${CONF_PATH}，输出空 features"
  printf "%s" ""
  exit 0
fi

# 输出：逗号分隔的 dep/feat token（无换行）
PYTHONPATH="${SCRIPT_DIR}" WATEROS_SCRIPTS_QUIET=1 python3 - <<'PY' "${ROOT_DIR}" "${CONF_PATH}" "${ROOT_PKG}"
from __future__ import annotations

import sys
from pathlib import Path

from source.feature_tools import (
    build_index,
    bubble_features_to_root,
    discover_manifests,
    parse_config_tree_enabled,
)

root_dir = Path(sys.argv[1]).resolve()
conf_path = Path(sys.argv[2]).resolve()

# root_pkg 默认为 root_dir/Cargo.toml 对应的 package name
root_pkg = sys.argv[3].strip() if len(sys.argv) >= 4 else ""
if not root_pkg:
    # 直接读取 root_dir/Cargo.toml 的 [package].name
    import tomllib
    root_manifest = root_dir / "Cargo.toml"
    data = tomllib.loads(root_manifest.read_text(encoding="utf-8"))
    root_pkg = data.get("package", {}).get("name", "").strip() or "wateros"

manifests = discover_manifests(root_dir)
crates, _ = build_index(manifests)
enabled = parse_config_tree_enabled(conf_path)
flags, warnings = bubble_features_to_root(root_pkg, enabled, crates)

for w in warnings:
    # In quiet mode (Makefile), keep stdout clean and suppress warning noise.
    import os as _os
    if _os.environ.get("WATEROS_SCRIPTS_QUIET") == "1":
        continue
    sys.stderr.write(f"WARN: {w}\n")

print(",".join(sorted(flags)), end="")
PY

