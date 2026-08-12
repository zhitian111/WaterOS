#!/bin/sh
# 扫描 os/ 下的 Cargo 清单，重新生成可编辑的 config.conf 与完整的
# feature-tree.txt。
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=/dev/null
. "${SCRIPT_DIR}/../source/console.bash"

ROOT_DIR_DEFAULT=$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)
ROOT_DIR="${1:-$ROOT_DIR_DEFAULT}"

info "开始生成 feature-tree.txt 与 config.conf"

# 复用原实现（保留老脚本兼容性）
"${SCRIPT_DIR}/export-feature-tree.bash" "${ROOT_DIR}"

info "完成：${ROOT_DIR}/feature-tree.txt"
info "完成：${ROOT_DIR}/config.conf"
