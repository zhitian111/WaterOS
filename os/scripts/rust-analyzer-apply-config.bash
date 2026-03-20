#!/bin/bash
set -eu

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/source/console.bash"

OS_DIR_DEFAULT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
OS_DIR="${1:-${OS_DIR_DEFAULT}}"

CONF_PATH="${OS_DIR}/config.conf"
if [[ ! -f "${CONF_PATH}" ]]; then
  error "找不到配置文件: ${CONF_PATH}（先运行 configure.bash）" 2
fi

# project root: .../WaterOS_refactor
WORKSPACE_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
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
        # next section
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

info "从 config.conf 生成 rust-analyzer features 并写入: ${SETTINGS_PATH}"

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

info "已更新 rust-analyzer.cargo.features。建议重启/Reload rust-analyzer。"

