#!/usr/bin/env bash
# RISC-V64 的 syscall-profile 构建、运行与分析入口。
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/syscall-profile.sh" rv "$@"
