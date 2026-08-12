#!/bin/bash
# 将生成的 feature 配置写入各 Cargo.toml 的默认项，或从相邻的 .wosbak
# 备份恢复。该脚本会修改清单文件，不属于日常构建步骤。
set -eu
WOS_LOG_COMPONENT=CONFIG

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/../source/console.bash"

OS_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  cat <<EOF
用法: ${0##*/} [OS_DIR] [apply|revert] [CONFIG_FILE]

将配置写入各 Cargo.toml 的 default features，或从 .wosbak 备份恢复。
  OS_DIR       OS 工程目录，默认为 ${OS_DIR_DEFAULT}
  apply        应用配置并创建备份，默认操作
  revert       恢复全部 Cargo.toml.wosbak
  CONFIG_FILE  相对 OS_DIR 的配置路径，默认为 config.conf

该脚本会批量修改 Cargo.toml，执行前应确认 Git 工作区状态。
EOF
  exit 0
fi
OS_DIR="${1:-${OS_DIR_DEFAULT}}"

MODE="${2:-apply}" # apply | revert

CONF_REL="${3:-config.conf}"
CONF_PATH="${OS_DIR}/${CONF_REL}"
if [[ ! -f "${CONF_PATH}" ]]; then
  error "配置文件不存在 path=${CONF_PATH} action=先运行_make_configure" 2
fi

BACKUP_SUFFIX=".wosbak"

if [[ "${MODE}" == "revert" ]]; then
  info "开始恢复 Cargo 默认 features backup_suffix=${BACKUP_SUFFIX}"
  python3 - <<'PY' "${OS_DIR}" "${BACKUP_SUFFIX}"
import shutil, sys
from pathlib import Path

os_dir = Path(sys.argv[1]).resolve()
suffix = sys.argv[2]

count = 0
for bak in os_dir.rglob(f"Cargo.toml{suffix}"):
    dst = bak.with_name("Cargo.toml")
    shutil.copy2(bak, dst)
    count += 1
print(f"reverted {count} files")
PY
  exit 0
fi

info "开始应用 Cargo 默认 features config=${CONF_PATH}"
info "即将修改 Cargo 清单 backup_suffix=${BACKUP_SUFFIX}"

python3 - <<'PY' "${CONF_PATH}" "${OS_DIR}" "${BACKUP_SUFFIX}"
from __future__ import annotations

import re
import shutil
import sys
from pathlib import Path
from typing import Dict, List, Set

os_dir = Path(sys.argv[2]).resolve()
backup_suffix = sys.argv[3]
sys.path.insert(0, str(os_dir / "scripts"))
sys.path.insert(0, str(os_dir / "scripts" / "source"))

from source.feature_tools import build_index, discover_manifests, parse_config_tree_enabled

conf_path = Path(sys.argv[1]).resolve()

manifests = discover_manifests(os_dir)
crates, pkg_to_manifest = build_index(manifests)

enabled_by_pkg = parse_config_tree_enabled(conf_path)

def quote_list(items: List[str]) -> str:
    return ", ".join([f"\"{x}\"" for x in items])

def update_default_features(manifest_path: Path, enabled_features: Set[str]) -> None:
    text = manifest_path.read_text(encoding="utf-8").splitlines(True)
    out: List[str] = []
    in_features = False
    features_indent = ""
    default_line_re = re.compile(r"^(?P<indent>\s*)default\s*=\s*\[.*\]\s*$")
    inserted = False

    # 根据第一条 feature 赋值确定候选缩进
    candidate_indent = "    "
    for i, line in enumerate(text):
        if line.strip() == "[features]":
            # 向后扫描，查找第一条带缩进的键赋值
            for j in range(i + 1, min(i + 30, len(text))):
                m = re.match(r"^(\s*)[A-Za-z0-9_-]+\s*=", text[j])
                if m:
                    candidate_indent = m.group(1) or candidate_indent
                    break
            break

    for line in text:
        if line.strip() == "[features]":
            in_features = True
            features_indent = line[: len(line) - len(line.lstrip(" \t"))]
            out.append(line)
            continue

        if in_features and re.match(r"^\s*\[.*\]\s*$", line) and line.strip() != "[features]":
            # 已离开 [features] 段
            if not inserted:
                items = sorted([x for x in enabled_features if x and x != "default"])
                if items:
                    new_line = f"{candidate_indent}default = [ {quote_list(items)} ]\n"
                else:
                    new_line = f"{candidate_indent}default = []\n"
                out.append(new_line)
                inserted = True
            in_features = False
            out.append(line)
            continue

        if in_features:
            m = default_line_re.match(line.rstrip("\n"))
            if m:
                items = sorted([x for x in enabled_features if x and x != "default"])
                indent = m.group("indent") or candidate_indent
                if items:
                    new_line = f"{indent}default = [ {quote_list(items)} ]\n"
                else:
                    new_line = f"{indent}default = []\n"
                out.append(new_line)
                inserted = True
                continue

        out.append(line)

    if in_features and not inserted:
        items = sorted([x for x in enabled_features if x and x != "default"])
        candidate_indent = candidate_indent
        if items:
            new_line = f"{candidate_indent}default = [ {quote_list(items)} ]\n"
        else:
            new_line = f"{candidate_indent}default = []\n"
        out.append(new_line)

    manifest_path.write_text("".join(out), encoding="utf-8")

# 在源码清单旁备份全部 Cargo.toml，确保恢复结果确定
for m in manifests:
    bak = m.with_name(f"{m.name}{backup_suffix}")
    shutil.copy2(m, bak)

# 1) Update [features] default in crates mentioned by config tree
for pkg, feats in enabled_by_pkg.items():
    crate = crates.get(pkg)
    if not crate:
        continue
    mp = Path(crate.manifest_path)
    update_default_features(mp, feats)

print(f"done. updated_crates={len(enabled_by_pkg)}")
PY

info "Cargo 默认 features 已更新 action=重新加载_rust-analyzer"
