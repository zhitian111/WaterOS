#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/source/console.bash"

ROOT_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
ROOT_DIR="${ROOT_DIR_DEFAULT}"

CONF_PATH="${1:-${ROOT_DIR}/config.conf}"

if [[ ! -f "${CONF_PATH}" ]]; then
  error "找不到配置文件: ${CONF_PATH}（先运行 configure.bash）" 2
fi

info "打印当前启用配置: ${CONF_PATH}"

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

        # only print enabled feature lines (already filtered comments)
        if not stack:
            continue
        current_crate = stack[-1][1]
        prefix = "/".join([c for _, c in stack])
        print(f"{prefix}: {line}")
PY

