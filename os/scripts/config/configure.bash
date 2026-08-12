#!/bin/sh
# 扫描 os/ 下的 Cargo 清单，重新生成可编辑的 config.conf 与完整的
# feature-tree.txt。
set -eu
WOS_LOG_COMPONENT=CONFIG

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=/dev/null
. "${SCRIPT_DIR}/../source/console.bash"

ROOT_DIR_DEFAULT=$(CDPATH= cd -- "${SCRIPT_DIR}/../.." && pwd)
if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  printf '用法: %s [OS_DIR]\n\n' "${0##*/}"
  printf '扫描 OS_DIR 下的 Cargo 清单，生成 config.conf 和 feature-tree.txt。\n'
  printf 'OS_DIR 默认为脚本所在的 os/ 目录。\n'
  exit 0
fi
ROOT_DIR="${1:-$ROOT_DIR_DEFAULT}"

info "开始生成 feature 配置 root=${ROOT_DIR}"

# 复用原实现（保留老脚本兼容性）
"${SCRIPT_DIR}/export-feature-tree.bash" "${ROOT_DIR}"

info "feature 树已生成 path=${ROOT_DIR}/feature-tree.txt"
info "feature 配置已生成 path=${ROOT_DIR}/config.conf"
