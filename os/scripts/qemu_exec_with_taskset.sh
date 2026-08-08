#!/usr/bin/env bash

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $(basename "$0") <qemu_binary> [args...]" >&2
    exit 1
fi

qemu_bin="$1"
shift

if [[ -n "${WOS_TASKSET_CPUS:-}" ]]; then
    if ! command -v taskset >/dev/null 2>&1; then
        echo "WOS_TASKSET_CPUS=$WOS_TASKSET_CPUS requested but 'taskset' not found" >&2
        exit 1
    fi
    echo "[qemu_exec_with_taskset] binding to cpus=${WOS_TASKSET_CPUS}: $qemu_bin $*" >&2
    exec taskset -c "$WOS_TASKSET_CPUS" "$qemu_bin" "$@"
fi

exec "$qemu_bin" "$@"
