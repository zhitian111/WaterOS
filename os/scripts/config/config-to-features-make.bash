#!/bin/bash
# 供 Make 与编辑器工具调用的安静适配层：从 config.conf 输出一行以逗号分隔的
# 顶层 feature 列表。
set -eu

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/../source/console.bash"

ROOT_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
ROOT_DIR="${ROOT_DIR_DEFAULT}"

CONF_PATH="${1:-${ROOT_DIR}/config.conf}"
ROOT_PKG="${2:-}"

# 只输出 features 内容（无前缀，无换行），日志走 stderr
WATEROS_SCRIPTS_QUIET=1 "${SCRIPT_DIR}/config-to-features.bash" "${CONF_PATH}" "${ROOT_PKG}"
