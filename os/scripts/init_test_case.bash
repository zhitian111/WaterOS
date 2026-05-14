#!/bin/bash
set -eu

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=/dev/null
source "${SCRIPT_DIR}/source/console.bash"

mkdir -p test_case
cd test_case
git clone https://github.com/oscomp/testsuits-for-oskernel.git .
git switch pre-2025


info "注意！请在 docker 启动后执行 make all"
sudo make docker

