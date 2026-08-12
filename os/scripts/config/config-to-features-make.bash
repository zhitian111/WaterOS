#!/bin/bash
# 供 Make 与编辑器工具调用的安静适配层：从 config.conf 输出一行以逗号分隔的
# 顶层 feature 列表。
set -eu

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/../source/console.bash"

ROOT_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
ROOT_DIR="${ROOT_DIR_DEFAULT}"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  cat <<EOF
用法: ${0##*/} [CONFIG_FILE] [ROOT_PACKAGE]

以安静模式输出逗号分隔的顶层 Cargo feature 列表，供 Makefile 和编辑器调用。
参数含义与 config-to-features.bash 相同。
EOF
  exit 0
fi

CONF_PATH="${1:-${ROOT_DIR}/config.conf}"
ROOT_PKG="${2:-}"

# 只输出 features 内容（无前缀，无换行），日志走 stderr
WATEROS_SCRIPTS_QUIET=1 "${SCRIPT_DIR}/config-to-features.bash" "${CONF_PATH}" "${ROOT_PKG}"
