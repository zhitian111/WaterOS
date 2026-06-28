#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
OS_DIR="$PWD"
BRINGUP="$OS_DIR/src/user_bringup_busybox.rs"
BACKUP="$BRINGUP.bak.phases"
LOG_DIR="/tmp/wateros_perf_phases"
mkdir -p "$LOG_DIR"

cp "$BRINGUP" "$BACKUP"
restore() { cp "$BACKUP" "$BRINGUP"; }
trap restore EXIT

apply_const() {
    local snippet="$1"
    python3 - "$BRINGUP" "$snippet" <<'PY'
import sys
path, snippet_path = sys.argv[1], sys.argv[2]
body = open(path).read()
snippet = open(snippet_path).read()
start = body.index('const BRINGUP_COMMANDS')
end = body.index('];', start) + 2
open(path, 'w').write(body[:start] + snippet + body[end:])
PY
}

run_phase() {
    local name="$1"
    local log="$LOG_DIR/${name}.log"
    echo "=== PHASE $name $(date -Is) ===" | tee -a "$LOG_DIR/summary.log"
    # 仅等待本 overlay 上的 QEMU 自然退出，避免 pkill 误杀其它并行任务。
    while pgrep -f "sdcard-rv\\.perf-${name}\\.overlay" >/dev/null 2>&1; do sleep 2; done
    make kernel-rv >>"$log" 2>&1
    WOS_SDCARD_BACKING=./sdcard-rv-local.img WOS_SNAPSHOT_ID="perf-${name}" \
        make rv_qemu_run_snapshot >>"$log" 2>&1 || true
    if grep -q 'all commands finished' "$log"; then
        echo "PHASE $name: OK" | tee -a "$LOG_DIR/summary.log"
    else
        echo "PHASE $name: INCOMPLETE" | tee -a "$LOG_DIR/summary.log"
        tail -3 "$log" | tee -a "$LOG_DIR/summary.log"
    fi
    grep -E '\[busybox-bringup\].*elapsed=' "$log" | tee -a "$LOG_DIR/summary.log"
    if grep -qiE 'Kernel panic|RefCell already borrowed' "$log"; then
        echo "PHASE $name: PANIC/REFCELL DETECTED" | tee -a "$LOG_DIR/summary.log"
        grep -iE 'Kernel panic|RefCell already borrowed' "$log" | head -5 | tee -a "$LOG_DIR/summary.log"
    fi
}

cat > "$LOG_DIR/p1.snippet" <<'EOF'
const BRINGUP_COMMANDS : &[BringupCommand] = &[
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/basic_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["sh", "/musl/basic_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/busybox_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["sh", "/musl/busybox_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/lua_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["sh", "/musl/lua_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/iperf_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["sh", "/musl/iperf_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/netperf_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["sh", "/musl/netperf_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["sh", "/musl/libctest_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/cyclictest_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["sh", "/musl/cyclictest_testcode.sh"] },
];
EOF

cat > "$LOG_DIR/p2.snippet" <<'EOF'
const BRINGUP_COMMANDS : &[BringupCommand] = &[
    BringupCommand { program : "/glibc/busybox", argv : &["timeout", "60", "sh", "/glibc/iozone_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["timeout", "60", "sh", "/musl/iozone_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["timeout", "90", "sh", "/glibc/libcbench_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["timeout", "90", "sh", "/musl/libcbench_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["timeout", "60", "sh", "/glibc/lmbench_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["timeout", "60", "sh", "/musl/lmbench_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox", argv : &["timeout", "90", "sh", "/glibc/unixbench_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["timeout", "90", "sh", "/musl/unixbench_testcode.sh"] },
];
EOF

cat > "$LOG_DIR/p3.snippet" <<'EOF'
const BRINGUP_COMMANDS : &[BringupCommand] = &[
    BringupCommand { program : "/glibc/busybox", argv : &["timeout", "600", "sh", "/glibc/ltp_testcode.sh"] },
    BringupCommand { program : "/musl/busybox", argv : &["timeout", "60", "sh", "/musl/ltp_testcode.sh"] },
];
EOF

apply_const "$LOG_DIR/p1.snippet"
run_phase p1_func

apply_const "$LOG_DIR/p2.snippet"
run_phase p2_bench

apply_const "$LOG_DIR/p3.snippet"
run_phase p3_ltp

echo "=== ALL PHASES DONE ===" | tee -a "$LOG_DIR/summary.log"
cat "$LOG_DIR/summary.log"
