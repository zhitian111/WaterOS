#!/usr/bin/env bash
# 最小验收：static check + RV/LO exec 冒烟（basic + busybox + lmbench Process）
# 脚本会临时替换 BRINGUP_COMMANDS，并通过 trap 在退出时恢复源文件。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
BRINGUP="$ROOT/src/user_bringup_busybox.rs"
BACKUP="$ROOT/src/user_bringup_busybox.rs.bak.min_accept"
LOG_DIR="/tmp/execve-lazy-min-accept"
mkdir -p "$LOG_DIR"

MIN_SNIPPET="$LOG_DIR/bringup.snippet"
cat > "$MIN_SNIPPET" <<'EOF'
const BRINGUP_COMMANDS : &[BringupCommand] = &[
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/basic_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/busybox_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["timeout", "120", "sh", "/glibc/lmbench_testcode.sh"] },
];
EOF

apply_bringup() {
    python3 - "$BRINGUP" "$MIN_SNIPPET" <<'PY'
import sys
path, snippet_path = sys.argv[1], sys.argv[2]
body = open(path).read()
snippet = open(snippet_path).read()
marker = '#[cfg(all(not(feature = "bringup-ltp-glibc-only"),\n          not(feature = "bringup-ltp-musl-only")))]'
start = body.index(marker)
start = body.index('const BRINGUP_COMMANDS', start)
end = body.index('];', start) + 2
open(path, 'w').write(body[:start] + snippet + body[end:])
PY
}

restore_bringup() {
    if [[ -f "$BACKUP" ]]; then
        cp "$BACKUP" "$BRINGUP"
        rm -f "$BACKUP"
    fi
}

run_rv() {
    local log="$LOG_DIR/rv.log"
    echo "=== RV minimal accept $(date -Is) ===" | tee "$LOG_DIR/summary.txt"
    make kernel-rv >>"$log" 2>&1
    WOS_SDCARD_BACKING=./sdcard-rv.img WOS_SNAPSHOT_ID="min-accept-rv" \
        make rv_qemu_run_snapshot >>"$log" 2>&1 || true
    analyze_log "$log" "RV"
}

run_la() {
    local log="$LOG_DIR/la.log"
    echo "=== LA minimal accept $(date -Is) ===" | tee -a "$LOG_DIR/summary.txt"
    make kernel-la >>"$log" 2>&1
    WOS_SDCARD_BACKING=./sdcard-la.img WOS_SNAPSHOT_ID="min-accept-la" \
        make la_qemu_run_snapshot >>"$log" 2>&1 || true
    # LA 仅跑 basic（busybox 整脚本较慢）
    analyze_log "$log" "LA-basic-only" "basic_testcode"
}

analyze_log() {
    local log="$1"
    local tag="$2"
    local filter="${3:-}"
    echo "--- $tag ---" | tee -a "$LOG_DIR/summary.txt"
    if grep -q 'all commands finished' "$log"; then
        echo "PASS: all commands finished" | tee -a "$LOG_DIR/summary.txt"
    else
        echo "FAIL: did not finish all commands" | tee -a "$LOG_DIR/summary.txt"
    fi
    if grep -qiE 'Kernel panic|Panicked at|RefCell already borrowed' "$log"; then
        echo "FAIL: panic detected" | tee -a "$LOG_DIR/summary.txt"
        grep -iE 'Kernel panic|Panicked at|RefCell already borrowed' "$log" | head -3 | tee -a "$LOG_DIR/summary.txt"
    else
        echo "PASS: no kernel panic" | tee -a "$LOG_DIR/summary.txt"
    fi
    if grep -q 'execve success' "$log"; then
        echo "PASS: test_execve (execve success)" | tee -a "$LOG_DIR/summary.txt"
    else
        echo "WARN: execve success not seen" | tee -a "$LOG_DIR/summary.txt"
    fi
    if grep -qE 'fork\+/bin/sh|fork.*/bin/sh' "$log"; then
        grep -E 'fork\+/bin/sh|fork.*/bin/sh' "$log" | head -3 | tee -a "$LOG_DIR/summary.txt"
    fi
    if [[ -n "$filter" ]]; then
        grep -E "\[busybox-bringup\].*elapsed=" "$log" | grep "$filter" | tee -a "$LOG_DIR/summary.txt" || true
    else
        grep -E '\[busybox-bringup\].*elapsed=' "$log" | tee -a "$LOG_DIR/summary.txt" || true
    fi
}

cp "$BRINGUP" "$BACKUP"
trap restore_bringup EXIT

apply_bringup
run_rv

# LA 只跑 basic
cat > "$MIN_SNIPPET" <<'EOF'
const BRINGUP_COMMANDS : &[BringupCommand] = &[
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/basic_testcode.sh"] },
];
EOF
apply_bringup
run_la

echo "=== DONE ===" | tee -a "$LOG_DIR/summary.txt"
cat "$LOG_DIR/summary.txt"
echo "完整日志目录: $LOG_DIR"
