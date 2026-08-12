#!/bin/bash
# 输出 config.conf 中某个 package 的直接 feature 选择。这是底层兼容工具；
# 生成顶层配置时优先使用 config-to-features.bash。
set -eu
WOS_LOG_COMPONENT=CONFIG

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/../source/console.bash"

if [[ "${WATEROS_SCRIPTS_QUIET:-0}" == "1" ]]; then
  trace() { :; }
  info() { :; }
  warning() { :; }
  debug() { :; }
fi

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  cat <<EOF
用法: ${0##*/} CONFIG_FILE PACKAGE

输出 CONFIG_FILE 中 PACKAGE 直接启用的 feature，结果以逗号分隔并写入 stdout。
该工具不沿依赖树传播 feature；生成顶层构建参数时应使用 config-to-features.bash。
EOF
  exit 0
fi

if [[ $# -lt 1 ]]; then
  error "参数不足 usage=${0##*/}_<config.conf>_<package> output=逗号分隔的_features" 2
fi

CONF_PATH="$1"
PKG="${2:-}"
if [[ ! -f "${CONF_PATH}" ]]; then
  error "配置文件不存在 path=${CONF_PATH}" 3
fi
if [[ -z "${PKG}" ]]; then
  error "缺少 package 参数 reason=config.conf_包含多个_crate" 4
fi

info "读取 feature 配置 path=${CONF_PATH} package=${PKG}"

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

        # crate 标题格式为 `name:`
        if line.endswith(":") and " " not in line[:-1]:
            name = line[:-1]
            if name == pkg:
                in_target = True
                target_indent = ind
                collect_indent = None
                enabled = []
                continue
            if in_target and target_indent is not None and ind <= target_indent:
                # 已离开目标块
                break
            continue

        if not in_target:
            continue

        if target_indent is None:
            continue

        # 第一条非标题行确定子项缩进
        if collect_indent is None:
            collect_indent = ind

        # 只收集缩进相同的直接子项
        if ind != collect_indent:
            # 更深的缩进属于嵌套 crate
            continue

        enabled.append(line)

if not in_target:
    sys.stderr.write(f"ERROR: 找不到 crate '{pkg}:' 段落\n")
    sys.exit(6)

print(",".join(enabled))
PY
)"

if [[ -z "${FEATURES}" ]]; then
  warning "未找到启用的 features package=${PKG} output=empty"
else
  info "Feature 配置生成完成 enabled=$(python3 - <<'PY' "${CONF_PATH}" "${PKG}"
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
