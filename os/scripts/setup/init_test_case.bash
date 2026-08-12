#!/bin/bash
# 克隆官方 testsuits-for-oskernel 测试仓库并启动其 Docker 初始化流程。
# 该脚本需要网络、Docker 与 sudo，会在当前目录创建 test_case/。
set -eu
WOS_LOG_COMPONENT=SETUP

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/../source/console.bash"

mkdir -p test_case
cd test_case
git clone https://github.com/oscomp/testsuits-for-oskernel.git .
git switch pre-2025


info "测试环境初始化完成 next=进入_Docker_后执行_make_all"
sudo make docker
