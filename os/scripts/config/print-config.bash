#!/bin/bash
# 按 crate 分组，以紧凑形式打印 config.conf 中启用的 features。
set -eu
WOS_LOG_COMPONENT=CONFIG

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/../source/console.bash"

ROOT_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
ROOT_DIR="${ROOT_DIR_DEFAULT}"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  cat <<EOF
用法: ${0##*/} [CONFIG_FILE]

按 crate 分组打印已启用的 feature。CONFIG_FILE 默认为 ${ROOT_DIR}/config.conf。
EOF
  exit 0
fi

CONF_PATH="${1:-${ROOT_DIR}/config.conf}"

if [[ ! -f "${CONF_PATH}" ]]; then
  error "配置文件不存在 path=${CONF_PATH} action=先运行_make_configure" 2
fi

info "读取 feature 配置 path=${CONF_PATH}"

python3 - <<'PY' "${CONF_PATH}"
import sys

path = sys.argv[1]

def indent_of(s: str) -> int:
    return len(s) - len(s.lstrip(" "))

stack = []  # list[(indent, crate)]

with open(path, "r", encoding="utf-8") as f:
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
            continue

        # 只输出已经过滤注释的启用 feature 行
        if not stack:
            continue
        current_crate = stack[-1][1]
        prefix = "/".join([c for _, c in stack])
        print(f"{prefix}: {line}")
PY
