#!/bin/bash
# 将 config.conf 转换为编辑器设置，使 rust-analyzer 使用相同的 feature 组合。
# 该脚本会写入工作区的 .cursor/settings.json。
set -eu
WOS_LOG_COMPONENT=CONFIG

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/../source/console.bash"

OS_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  cat <<EOF
用法: ${0##*/} [OS_DIR]

读取 OS_DIR/config.conf，并把 rust-analyzer Cargo features 写入
OS_DIR/.cursor/settings.json。OS_DIR 默认为 ${OS_DIR_DEFAULT}。
EOF
  exit 0
fi
OS_DIR="${1:-${OS_DIR_DEFAULT}}"

CONF_PATH="${OS_DIR}/config.conf"
if [[ ! -f "${CONF_PATH}" ]]; then
  error "配置文件不存在 path=${CONF_PATH} action=先运行_make_configure" 2
fi

# 项目根目录：.../WaterOS_refactor
WORKSPACE_ROOT="$(cd -- "${SCRIPT_DIR}/../../.." && pwd)"
CURSOR_DIR="${WORKSPACE_ROOT}/.cursor"
SETTINGS_PATH="${CURSOR_DIR}/settings.json"

mkdir -p "${CURSOR_DIR}"

ROOT_PKG="$(
  python3 - <<'PY' "${OS_DIR}/Cargo.toml"
import re, sys

path = sys.argv[1]
text = open(path, 'r', encoding='utf-8').read().splitlines()

in_package = False
root = ''
for line in text:
    s = line.strip()
    if s == '[package]':
        in_package = True
        continue
    if in_package and s.startswith('[') and s.endswith(']'):
        # 已进入下一个段落
        break
        if in_package:
            m = re.match(r"^name\\s*=\\s*(['\\\"])(.*?)\\1\\s*$", s)
        if m:
            root = m.group(2).strip()
            break

print(root)
PY
)"
if [[ -z "${ROOT_PKG}" ]]; then
  error "无法从 ${OS_DIR}/Cargo.toml 读取 package.name" 3
fi

info "开始更新 rust-analyzer features config=${CONF_PATH} output=${SETTINGS_PATH}"

FEATURES_CSV="$(
  WATEROS_SCRIPTS_QUIET=1 "${SCRIPT_DIR}/config-to-features-make.bash" "${CONF_PATH}" "${ROOT_PKG}"
)"

python3 - <<'PY' "${SETTINGS_PATH}" "${FEATURES_CSV}"
import json
import sys
from pathlib import Path

settings_path = Path(sys.argv[1])
csv = sys.argv[2].strip()
arr = [s for s in csv.split(",") if s] if csv else []

data = {}
if settings_path.exists():
    try:
        data = json.loads(settings_path.read_text(encoding="utf-8"))
    except Exception:
        data = {}

data["rust-analyzer.cargo.features"] = arr

settings_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print("ok")
PY

info "rust-analyzer features 已更新 action=重新加载_rust-analyzer"
