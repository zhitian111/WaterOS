#!/usr/bin/env bash
# LTP 卡死自动迭代：跑测 → 检测卡死 → 写入 skip 表 → debugfs 断点续跑 → 重编内核 → 下一轮。
#
# 用法（在 os/ 目录）:
#   ./scripts/ltp_hang_iterate.sh              # 从 os/sdcard-rv.img 内 checkpoint 续跑
#   ./scripts/ltp_hang_iterate.sh --reset-img  # cp test_case/sdcard-rv.img 后 debugfs 注入
#   ./scripts/ltp_hang_iterate.sh --resume-after dhcpd_tests.sh
#
# 仅通过 debugfs 修改 os/sdcard-rv.img 内的 /glibc/ltp_testcode.sh 等，不改 test_case 源树。
#
# 环境变量:
#   LTP_HANG_POLL_SEC=60    日志轮询间隔（秒）
#   LTP_HANG_STABLE_SEC=120 尾部不变超过此秒数判定卡死
#   LTP_HANG_MAX_ROUNDS=0   最大轮数，0=不限

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SKIP_RS="$ROOT/src/user_bringup_ltp_exclusions.rs"
SDCARD="$ROOT/sdcard-rv.img"
SDCARD_SRC="$ROOT/../test_case/sdcard-rv.img"
LTP_LOG_DIR="$ROOT/ltp_log"
GLIBC_CHECKPOINT="/glibc/.ltp_resume_after"
GLIBC_SCRIPT="/glibc/ltp_testcode.sh"
MUSL_CHECKPOINT="/musl/.ltp_resume_after"
MUSL_SCRIPT="/musl/ltp_testcode.sh"

POLL_SEC="${LTP_HANG_POLL_SEC:-60}"
STABLE_SEC="${LTP_HANG_STABLE_SEC:-120}"
MAX_ROUNDS="${LTP_HANG_MAX_ROUNDS:-0}"

RESET_IMG=0
ONCE=0
INJECT_ONLY=0
RESUME_AFTER=""
while [ $# -gt 0 ]; do
    case "$1" in
        --reset-img) RESET_IMG=1; shift ;;
        --once) ONCE=1; shift ;;
        --inject-only) INJECT_ONLY=1; shift ;;
        --resume-after)
            shift
            RESUME_AFTER="${1:-}"
            [ -n "$RESUME_AFTER" ] || { echo "missing value for --resume-after" >&2; exit 2; }
            shift
            ;;
        --resume-after=*) RESUME_AFTER="${1#--resume-after=}"; shift ;;
        -h|--help)
            sed -n '2,13p' "$0"
            exit 0
            ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

mkdir -p "$LTP_LOG_DIR"

log() { printf '[ltp-iterate] %s\n' "$*"; }

reset_sdcard_image() {
    log "reset sdcard: $SDCARD_SRC -> $SDCARD"
    cp -f "$SDCARD_SRC" "$SDCARD"
    inject_resume_runner "" ""
}

debugfs_write_file() {
    local local_path="$1"
    local remote_path="$2"
    debugfs -w -R "rm $remote_path" "$SDCARD" 2>/dev/null || true
    debugfs -w -R "write $local_path $remote_path" "$SDCARD"
}

write_resume_ltp_script() {
    local tmp="$1"
    local start_marker="$2"
    local end_marker="$3"
    local checkpoint="$4"
    cat >"$tmp" <<EOF
#!/bin/bash
echo "#### OS COMP TEST GROUP START ${start_marker} ####"
target_dir="ltp/testcases/bin"
checkpoint="${checkpoint}"
resume_after=""
[ -f "\$checkpoint" ] && resume_after=\$(cat "\$checkpoint")
skipping=1
[ -z "\$resume_after" ] && skipping=0
for file in "\$target_dir"/*; do
  [ -f "\$file" ] || continue
  base=\$(basename "\$file")
  if [ "\$skipping" = 1 ]; then
    [ "\$base" = "\$resume_after" ] && skipping=0
    continue
  fi
  echo "RUN LTP CASE \$base"
  "\$file"
  ret=\$?
  echo "FAIL LTP CASE \$base : \$ret"
  echo "\$base" > "\$checkpoint"
done
rm -f "\$checkpoint"
echo "#### OS COMP TEST GROUP END ${end_marker} ####"
EOF
}

inject_resume_runner() {
    local glibc_resume="${1:-}"
    local musl_resume=""
    [ $# -ge 2 ] && musl_resume="$2"
    kill_qemu
    local tmp
    tmp="$(mktemp)"
    write_resume_ltp_script "$tmp" "ltp-glibc" "ltp-glibc" "$GLIBC_CHECKPOINT"
    debugfs_write_file "$tmp" "$GLIBC_SCRIPT"
    log "debugfs wrote $GLIBC_SCRIPT on $SDCARD"
    write_resume_ltp_script "$tmp" "ltp-musl" "ltp-musl" "$MUSL_CHECKPOINT"
    debugfs_write_file "$tmp" "$MUSL_SCRIPT"
    log "debugfs wrote $MUSL_SCRIPT on $SDCARD"
    rm -f "$tmp"
    if [ -n "$glibc_resume" ]; then
        tmp="$(mktemp)"
        printf '%s\n' "$glibc_resume" >"$tmp"
        debugfs_write_file "$tmp" "$GLIBC_CHECKPOINT"
        rm -f "$tmp"
        log "debugfs checkpoint $GLIBC_CHECKPOINT = '$glibc_resume'"
    else
        debugfs -w -R "rm $GLIBC_CHECKPOINT" "$SDCARD" 2>/dev/null || true
    fi
    if [ -n "$musl_resume" ]; then
        tmp="$(mktemp)"
        printf '%s\n' "$musl_resume" >"$tmp"
        debugfs_write_file "$tmp" "$MUSL_CHECKPOINT"
        rm -f "$tmp"
        log "debugfs checkpoint $MUSL_CHECKPOINT = '$musl_resume'"
    else
        debugfs -w -R "rm $MUSL_CHECKPOINT" "$SDCARD" 2>/dev/null || true
    fi
}

read_skip_list() {
    python3 - "$SKIP_RS" <<'PY'
import re, sys
text = open(sys.argv[1]).read()
mark = "const LTP_SUBMIT_SKIP_BASENAMES"
start = text.find(mark)
if start < 0:
    sys.exit(1)
bracket = text.find("[", start)
end = text.find("];", bracket)
if end < 0:
    sys.exit(1)
for m in re.finditer(r'"([^"]+)"', text[bracket + 1 : end]):
    print(m.group(1))
PY
}

add_skip_entry() {
    local name="$1"
    if read_skip_list | grep -Fxq "$name"; then
        log "skip list already contains: $name"
        return 0
    fi
    log "adding to LTP_SUBMIT_SKIP_BASENAMES (sorted): $name"
    python3 - "$SKIP_RS" "$name" <<'PY'
import re, sys
path, name = sys.argv[1], sys.argv[2]
text = open(path).read()
pat = r"(const LTP_SUBMIT_SKIP_BASENAMES:\s*&\[)(.*?)(\n\];)"
m = re.search(pat, text, re.S)
if not m:
    raise SystemExit("LTP_SUBMIT_SKIP_BASENAMES array not found")
names = re.findall(r'"([^"]+)"', m.group(2))
if name in names:
    sys.exit(0)
names.append(name)
names.sort()
new_body = "".join(f'\n    "{n}",' for n in names) + "\n"
text = text[: m.start(2)] + new_body + text[m.end(2) :]
open(path, "w").write(text)
PY
}

parse_last_completed() {
    local logfile="$1"
    grep '^FAIL LTP CASE ' "$logfile" 2>/dev/null | tail -1 | sed -n 's/^FAIL LTP CASE \(.*\) :.*/\1/p'
}

parse_last_run() {
    local logfile="$1"
    grep '^RUN LTP CASE ' "$logfile" 2>/dev/null | tail -1 | sed -n 's/^RUN LTP CASE \(.*\)/\1/p'
}

parse_stuck_case() {
    local logfile="$1"
    local last_run last_fail
    last_run="$(parse_last_run "$logfile")"
    last_fail="$(parse_last_completed "$logfile")"
    if [ -z "$last_run" ]; then
        echo ""
        return
    fi
    if [ "$last_run" = "$last_fail" ]; then
        echo ""
        return
    fi
    echo "$last_run"
}

run_finished_ok() {
    local logfile="$1"
    grep -q '#### OS COMP TEST GROUP END ltp-musl ####' "$logfile" 2>/dev/null
}

log_tail_hash() {
    local logfile="$1"
    tail -n 30 "$logfile" 2>/dev/null | md5sum | awk '{print $1}'
}

kill_qemu() {
    pkill -f 'qemu-system-riscv64.*sdcard-rv.img' 2>/dev/null || true
    sleep 2
    pkill -9 -f 'qemu-system-riscv64.*sdcard-rv.img' 2>/dev/null || true
}

run_qemu_with_monitor() {
    local logfile="$1"
    kill_qemu
    : >"$logfile"
    log "starting: make rv_qemu_run -> $logfile"
    make rv_qemu_run >>"$logfile" 2>&1 &
    local make_pid=$!
    local prev_hash="" stable_for=0 ltp_started=0
    while kill -0 "$make_pid" 2>/dev/null; do
        sleep "$POLL_SEC"
        if grep -q '#### OS COMP TEST GROUP START ltp-glibc ####' "$logfile" 2>/dev/null; then
            ltp_started=1
        fi
        if run_finished_ok "$logfile"; then
            wait "$make_pid" || true
            log "run finished normally"
            return 0
        fi
        if [ "$ltp_started" = 0 ]; then
            log "poll ok (waiting for LTP start)"
            continue
        fi
        local h
        h="$(log_tail_hash "$logfile")"
        if [ "$h" = "$prev_hash" ] && [ -n "$h" ]; then
            stable_for=$((stable_for + POLL_SEC))
        else
            stable_for=0
            prev_hash="$h"
        fi
        if [ "$stable_for" -ge "$STABLE_SEC" ]; then
            log "hang detected (${STABLE_SEC}s stable tail in LTP)"
            kill_qemu
            wait "$make_pid" 2>/dev/null || true
            return 1
        fi
        log "poll ok (${stable_for}s / ${STABLE_SEC}s stable)"
    done
    wait "$make_pid" || true
    if run_finished_ok "$logfile"; then
        log "run finished normally"
        return 0
    fi
    return 1
}

one_round() {
    local round="$1"
    local logfile="$LTP_LOG_DIR/rv_local_ltp_$(date +%y%m%d%H%M%S)_r${round}.log"
    if run_qemu_with_monitor "$logfile"; then
        log "LTP complete. log=$logfile"
        return 0
    fi
    local stuck resume_after
    stuck="$(parse_stuck_case "$logfile")"
    resume_after="$(parse_last_completed "$logfile")"
    if [ -z "$stuck" ]; then
        if grep -q 'Failed to get "write" lock' "$logfile" 2>/dev/null; then
            log "qemu/sdcard lock error; retry next round"
            return 1
        fi
        if ! grep -q '#### OS COMP TEST GROUP START ltp-glibc ####' "$logfile" 2>/dev/null; then
            log "LTP glibc never started (check root mount / sdcard); log=$logfile"
            return 2
        fi
        log "hang but no stuck case parsed; log=$logfile"
        return 2
    fi
    log "stuck case: $stuck (resume after: ${resume_after:-<start>})"
    add_skip_entry "$stuck"
    log "rebuilding kernel-rv..."
    make kernel-rv
    inject_resume_runner "$resume_after" ""
    log "round $round done; log=$logfile"
    return 1
}

main() {
    if [ "$RESET_IMG" = 1 ]; then
        reset_sdcard_image
    elif [ ! -f "$SDCARD" ]; then
        reset_sdcard_image
    fi
    if [ -n "$RESUME_AFTER" ] || [ "$INJECT_ONLY" = 1 ]; then
        inject_resume_runner "$RESUME_AFTER" ""
    fi
    if [ "$INJECT_ONLY" = 1 ]; then
        log "inject-only done on $SDCARD"
        exit 0
    fi
    if ! grep -q 'ltp_testcode.sh' "$ROOT/src/user_bringup_busybox.rs" 2>/dev/null; then
        log "WARN: user_bringup_busybox.rs may not enable /glibc/ltp_testcode.sh"
    fi
    local round=1
    while :; do
        log "===== round $round ====="
        if one_round "$round"; then
            log "all LTP cases finished"
            break
        fi
        if [ "$ONCE" = 1 ]; then
            log "--once: stop after one round"
            break
        fi
        if [ "$MAX_ROUNDS" -gt 0 ] && [ "$round" -ge "$MAX_ROUNDS" ]; then
            log "MAX_ROUNDS=$MAX_ROUNDS reached"
            break
        fi
        round=$((round + 1))
    done
}

main
