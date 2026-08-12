#!/usr/bin/env bash
# 执行给定的 QEMU 命令；设置 WOS_TASKSET_CPUS 时通过 taskset 绑定宿主 CPU。
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WOS_LOG_COMPONENT=QEMU
# shellcheck source=/dev/null
source "$SCRIPT_DIR/../source/console.bash"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    cat <<EOF
用法: ${0##*/} QEMU_BINARY [QEMU_ARG ...]

执行指定的 QEMU 命令。设置 WOS_TASKSET_CPUS 后，通过 taskset 将进程绑定到
对应的宿主 CPU 列表，例如 WOS_TASKSET_CPUS=0-3。
EOF
    exit 0
fi

if [[ $# -lt 1 ]]; then
    error "缺少 QEMU 命令 usage=$(basename "$0")_<qemu_binary>_[args...]" 2
fi

qemu_bin="$1"
shift

if [[ -n "${WOS_TASKSET_CPUS:-}" ]]; then
    if ! command -v taskset >/dev/null 2>&1; then
        error "无法绑定宿主 CPU cpuset=${WOS_TASKSET_CPUS} reason=taskset_not_found" 1
    fi
    info "绑定宿主 CPU cpuset=${WOS_TASKSET_CPUS} command=${qemu_bin}"
    exec taskset -c "$WOS_TASKSET_CPUS" "$qemu_bin" "$@"
fi

exec "$qemu_bin" "$@"
