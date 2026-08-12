#!/bin/bash
# 分三组运行 LoongArch64 功能、benchmark 与 LTP 性能负载。
# 脚本会临时替换 BRINGUP_COMMANDS，使用 qcow2 overlay，并在退出时恢复源码。
set -euo pipefail
cd "$(dirname "$0")/../.."
OS_DIR="$PWD"
WOS_LOG_COMPONENT=PERF
# shellcheck source=/dev/null
source "$OS_DIR/scripts/source/console.bash"
BRINGUP="$OS_DIR/src/user_bringup_busybox.rs"
BACKUP="$BRINGUP.bak.phases.la"
LOG_DIR="/tmp/wateros_perf_phases_la"
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
marker = '#[cfg(all(not(feature = "bringup-ltp-glibc-only"),'
idx = body.index(marker)
start = body.index('const BRINGUP_COMMANDS', idx)
end = body.index('];', start) + 2
open(path, 'w').write(body[:start] + snippet + body[end:])
PY
}

run_phase() {
    local name="$1"
    local log="$LOG_DIR/${name}.log"
    info "开始运行性能阶段 phase=${name} timestamp=$(date -Is) log=${log}"
    while pgrep -f "sdcard-la\\.perf-${name}\\.overlay" >/dev/null 2>&1; do sleep 2; done
    make kernel-la >>"$log" 2>&1
    WOS_SDCARD_BACKING=./sdcard-la-local.img WOS_SNAPSHOT_ID="perf-${name}" \
        make la_qemu_run_snapshot >>"$log" 2>&1 || true
    if grep -q 'all commands finished' "$log"; then
        printf '[PERF][INFO] 性能阶段完成 phase=%s status=passed\n' "$name" | tee -a "$LOG_DIR/summary.log"
    else
        printf '[PERF][WARN] 性能阶段未完成 phase=%s status=incomplete\n' "$name" | tee -a "$LOG_DIR/summary.log"
        tail -3 "$log" | tee -a "$LOG_DIR/summary.log"
    fi
    grep -E '\[busybox-bringup\].*elapsed=' "$log" | tee -a "$LOG_DIR/summary.log"
    if grep -qiE 'Kernel panic|RefCell already borrowed' "$log"; then
        printf '[PERF][ERROR] 性能阶段检测到内核错误 phase=%s reason=panic_or_refcell\n' "$name" | tee -a "$LOG_DIR/summary.log"
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

printf '[PERF][INFO] 全部性能阶段结束 log_dir=%s\n' "$LOG_DIR" | tee -a "$LOG_DIR/summary.log"
cat "$LOG_DIR/summary.log"
