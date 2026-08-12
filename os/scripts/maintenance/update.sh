#!/usr/bin/env bash
# 兼容旧入口的 Git 提交辅助脚本，实际逻辑由 update.bash 提供。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/update.bash" "$@"
