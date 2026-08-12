#!/usr/bin/env bash
# 在宿主 CPU 预算内并发执行多条独立 QEMU 命令，并为每个任务单独保存日志。
# 每条命令应作为一个带引号的独立参数传入。
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage:
  run_qemu_parallel.sh <command-1> [command-2 ...]

Environment:
  WOS_HOST_CPUS             Host CPU count used for planning. Default: nproc.
  WOS_CORES_PER_JOB         Host core count for one qemu instance. Default: 8.
  WOS_AUTO_SMP              When command does not set WOS_SMP, inject
                            WOS_SMP=<WOS_CORES_PER_JOB>. Default: 0.
  WOS_AUTO_UNLOCK_DRIVE    When set to 1, inject
                            WOS_QEMU_IMAGE_DRIVE_OPTIONS=locking=off.
                            Auto-applies only for qcow2-compatible images.
  WOS_MAX_PARALLEL_JOBS     Max parallel jobs. Default: floor(WOS_HOST_CPUS / WOS_CORES_PER_JOB).
  WOS_QEMU_PARALLEL_LOG_DIR Log directory. Default: ./tmp/wateros-qemu-parallel.
  WOS_QEMU_PARALLEL_WORKDIR Working directory for all commands. Default: current directory.

Example:
  WOS_CORES_PER_JOB=4 WOS_AUTO_SMP=1 ./scripts/run/run_qemu_parallel.sh \
    "WOS_SMP=4 make rv_final_run" \
    "WOS_SMP=4 make rv_final_run"
  # 或者使用自动注入：
  WOS_CORES_PER_JOB=4 WOS_AUTO_SMP=1 ./scripts/run/run_qemu_parallel.sh \
    "make rv_final_run" \
    "make rv_final_run"
USAGE
}

if [[ $# -lt 1 ]]; then
    usage
    exit 1
fi

HOST_CPUS="${WOS_HOST_CPUS:-$(nproc)}"
CORES_PER_JOB="${WOS_CORES_PER_JOB:-8}"
AUTO_SMP="${WOS_AUTO_SMP:-0}"
AUTO_UNLOCK_DRIVE="${WOS_AUTO_UNLOCK_DRIVE:-0}"
MAX_PARALLEL="${WOS_MAX_PARALLEL_JOBS:-0}"
LOG_DIR="${WOS_QEMU_PARALLEL_LOG_DIR:-./tmp/wateros-qemu-parallel}"
WORKDIR="${WOS_QEMU_PARALLEL_WORKDIR:-$(pwd)}"
SAFE_WORKDIR=$(printf "%q" "$WORKDIR")

if ! [[ "$HOST_CPUS" =~ ^[0-9]+$ ]] || (( HOST_CPUS <= 0 )); then
    echo "WOS_HOST_CPUS must be a positive integer" >&2
    exit 1
fi
if ! [[ "$CORES_PER_JOB" =~ ^[0-9]+$ ]] || (( CORES_PER_JOB <= 0 )); then
    echo "WOS_CORES_PER_JOB must be a positive integer" >&2
    exit 1
fi
if (( CORES_PER_JOB > HOST_CPUS )); then
    echo "WOS_CORES_PER_JOB=$CORES_PER_JOB is larger than host CPUs $HOST_CPUS" >&2
    exit 1
fi

MAX_SLOT_CALC=$((HOST_CPUS / CORES_PER_JOB))
if (( MAX_SLOT_CALC < 1 )); then
    echo "Host CPUs $HOST_CPUS too small for CORES_PER_JOB $CORES_PER_JOB" >&2
    exit 1
fi

if (( MAX_PARALLEL <= 0 || MAX_PARALLEL > MAX_SLOT_CALC )); then
    MAX_PARALLEL=$MAX_SLOT_CALC
fi

total_cmds=$#
if (( total_cmds < MAX_PARALLEL )); then
    MAX_PARALLEL=$total_cmds
fi

mkdir -p "$LOG_DIR"

commands=("$@")
declare -a slot_pids=()
declare -a slot_cmds=()
declare -a all_pids=()

extract_smp() {
    local cmd="$1"
    if [[ "$cmd" =~ (^|[[:space:]])WOS_SMP=([0-9]+) ]]; then
        echo "${BASH_REMATCH[2]}"
        return 0
    fi
    echo ""
}

extract_sdcard() {
    local cmd="$1"
    if [[ "$cmd" =~ (^|[[:space:]])WOS_SDCARD=([^[:space:]]+) ]]; then
        echo "${BASH_REMATCH[2]}"
        return 0
    fi
    if [[ -n "${WOS_SDCARD:-}" ]]; then
        echo "$WOS_SDCARD"
        return 0
    fi
    echo "./sdcard-rv.img"
}

supports_locking_off() {
    local image="$1"
    case "$image" in
        *.qcow|*.qcow2)
            return 0
            ;;
    esac
    return 1
}

acquire_slot() {
    local slot=""
    while true; do
        for i in $(seq 0 $((MAX_PARALLEL - 1))); do
            pid="${slot_pids[$i]:-}"
            if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
                slot="$i"
                break
            fi
        done
        if [[ -n "$slot" ]]; then
            echo "$slot"
            return 0
        fi
        wait -n
    done
}

for idx in "${!commands[@]}"; do
    slot="$(acquire_slot)"
    start_cpu=$((slot * CORES_PER_JOB))
    end_cpu=$((start_cpu + CORES_PER_JOB - 1))
    if (( end_cpu >= HOST_CPUS )); then
        end_cpu=$((HOST_CPUS - 1))
    fi
    cpuset="${start_cpu}-${end_cpu}"
    smp=""

    parsed_smp="$(extract_smp "${commands[$idx]}")"
    if [[ -n "$parsed_smp" ]]; then
        smp="$parsed_smp"
    elif [[ "$AUTO_SMP" == "1" ]]; then
        smp="$CORES_PER_JOB"
        commands["$idx"]="WOS_SMP=${smp} ${commands[$idx]}"
    fi
    if [[ "$AUTO_UNLOCK_DRIVE" == "1" ]]; then
        image_path="$(extract_sdcard "${commands[$idx]}")"
        if ! supports_locking_off "$image_path"; then
            echo "[INFO] skip locking=off for $image_path: not qcow2-compatible"
            :
        elif [[ "${commands[$idx]}" != *"WOS_QEMU_IMAGE_DRIVE_OPTIONS="* ]]; then
            commands["$idx"]="WOS_QEMU_IMAGE_DRIVE_OPTIONS=locking=off ${commands[$idx]}"
        fi
    fi

    if [[ -n "$smp" && "$smp" -gt "$CORES_PER_JOB" ]]; then
        echo "[WARN] command #$((idx + 1)) uses WOS_SMP=$smp but CORES_PER_JOB=$CORES_PER_JOB (may oversubscribe host cores)"
    fi

    log_file="$LOG_DIR/qemu-parallel-$(printf "%02d" "$idx").log"
    cmd="${commands[$idx]}"

    echo "[START] #$((idx + 1)) slot=$slot cpuset=${cpuset} smp=${smp:-unknown} cmd=${cmd}"
    echo "[LOG] ${log_file}"

    (WOS_TASKSET_CPUS="$cpuset" bash -lc "cd $SAFE_WORKDIR && $cmd") >"$log_file" 2>&1 &
    pid=$!
    slot_pids["$slot"]=$pid
    slot_cmds["$slot"]=$cmd
    all_pids+=("$pid")
done

cleanup() {
    for pid in "${all_pids[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            kill -TERM "$pid" 2>/dev/null || true
        fi
    done
}
trap cleanup EXIT INT TERM

status=0
for pid in "${all_pids[@]}"; do
    if ! wait "$pid"; then
        status=1
    fi
done

if (( status != 0 )); then
    echo "[RESULT] one or more jobs failed. See logs under ${LOG_DIR}" >&2
    exit 1
fi

echo "[RESULT] all jobs completed successfully. Logs in ${LOG_DIR}"
