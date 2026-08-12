#!/bin/bash
# 克隆官方 testsuits-for-oskernel 测试仓库并启动其 Docker 初始化流程。
# 该脚本需要网络、Docker 与 sudo，会在当前目录创建 test_case/。
set -eu

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/../source/console.bash"

mkdir -p test_case
cd test_case
git clone https://github.com/oscomp/testsuits-for-oskernel.git .
git switch pre-2025


info "注意！请在 docker 启动后执行 make all"
sudo make docker
