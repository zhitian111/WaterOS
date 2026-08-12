#!/usr/bin/env bash
# RISC-V64 的 wait-hot 入口。
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/wait-hot.sh" rv "$@"
