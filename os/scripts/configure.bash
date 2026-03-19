#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/source/console.bash"

ROOT_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
ROOT_DIR="${1:-${ROOT_DIR_DEFAULT}}"

info "开始生成 feature-tree.txt 与 config.conf"

# 复用原实现（保留老脚本兼容性）
"${SCRIPT_DIR}/export-feature-tree.bash" "${ROOT_DIR}"

info "完成：${ROOT_DIR}/feature-tree.txt"
info "完成：${ROOT_DIR}/config.conf"

