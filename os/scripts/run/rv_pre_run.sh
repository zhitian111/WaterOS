#!/bin/bash
# RISC-V64 pre 阶段的兼容启动入口；新流程优先使用 `make run`。
set -euo pipefail
exec python3 "$(dirname "$0")/qemu_run.py" --arch rv --profile pre
