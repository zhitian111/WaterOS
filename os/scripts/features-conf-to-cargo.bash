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

if [[ $# -lt 1 ]]; then
  error "用法: ${0##*/} <config.conf> <package>  （输出：--features 后面的逗号分隔字符串）" 2
fi

CONF_PATH="$1"
PKG="${2:-}"
if [[ ! -f "${CONF_PATH}" ]]; then
  error "配置文件不存在: ${CONF_PATH}" 3
fi
if [[ -z "${PKG}" ]]; then
  error "必须指定 package（因为 config.conf 是一棵树）" 4
fi

info "读取配置文件: ${CONF_PATH}"

# 规则：
# - 支持缩进树：crate 行以 ':' 结尾；其下更深缩进的是内容
# - 找到目标 crate 行后，收集其“直接子层级”的非注释行作为 feature token
FEATURES="$(
  python3 - <<'PY' "${CONF_PATH}" "${PKG}"
import sys

path = sys.argv[1]
pkg = (sys.argv[2] or "").strip()

def indent_of(s: str) -> int:
    return len(s) - len(s.lstrip(" "))

target_indent = None
collect_indent = None
enabled = []
in_target = False

with open(path, "r", encoding="utf-8") as f:
    for raw in f:
        if not raw.strip():
            continue
        if raw.lstrip().startswith("#"):
            continue
        ind = indent_of(raw.rstrip("\n"))
        line = raw.strip()

        # crate header: "name:"
        if line.endswith(":") and " " not in line[:-1]:
            name = line[:-1]
            if name == pkg:
                in_target = True
                target_indent = ind
                collect_indent = None
                enabled = []
                continue
            if in_target and target_indent is not None and ind <= target_indent:
                # left target block
                break
            continue

        if not in_target:
            continue

        if target_indent is None:
            continue

        # first non-header line sets child indent
        if collect_indent is None:
            collect_indent = ind

        # only collect direct children (same indent)
        if ind != collect_indent:
            # deeper indent belongs to nested crate's content
            continue

        enabled.append(line)

if not in_target:
    sys.stderr.write(f"ERROR: 找不到 crate '{pkg}:' 段落\n")
    sys.exit(6)

print(",".join(enabled))
PY
)"

if [[ -z "${FEATURES}" ]]; then
  warning "没有任何启用项（输出为空字符串）"
else
  info "启用项数量: $(python3 - <<'PY' "${CONF_PATH}" "${PKG}"
import sys
path = sys.argv[1]
pkg = (sys.argv[2] or "").strip()

def indent_of(s: str) -> int:
    return len(s) - len(s.lstrip(" "))

target_indent = None
collect_indent = None
enabled = []
in_target = False

with open(path, "r", encoding="utf-8") as f:
    for raw in f:
        if not raw.strip():
            continue
        if raw.lstrip().startswith("#"):
            continue
        ind = indent_of(raw.rstrip("\n"))
        line = raw.strip()
        if line.endswith(":") and " " not in line[:-1]:
            name = line[:-1]
            if name == pkg:
                in_target = True
                target_indent = ind
                collect_indent = None
                enabled = []
                continue
            if in_target and target_indent is not None and ind <= target_indent:
                break
            continue
        if not in_target or target_indent is None:
            continue
        if collect_indent is None:
            collect_indent = ind
        if ind != collect_indent:
            continue
        enabled.append(line)

print(len(enabled))
PY
)"
fi

printf "%s" "${FEATURES}"

