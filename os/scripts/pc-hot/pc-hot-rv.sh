#!/usr/bin/env bash
# RISC-V64 的 pc-hot 入口，用于逐 PC 指令计数和符号聚合。
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/pc-hot.sh" rv "$@"
