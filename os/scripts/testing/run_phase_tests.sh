#!/usr/bin/env bash
# 按阶段跑 QEMU 测试（每次只启用一个 P* 分组，避免未实现 syscall panic 拖垮后续用例）
# 脚本会临时改写用户态 bring-up 列表，并在退出时恢复原文件。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WOS_LOG_COMPONENT=TEST
# shellcheck source=/dev/null
source "$ROOT/scripts/source/console.bash"
BRINGUP="$ROOT/src/user_bringup_busybox.rs"
BACKUP="$ROOT/src/user_bringup_busybox.rs.bak"
PARSER="$ROOT/scripts/testing/parse_qemu_test_log.py"
LOG_DIR="/tmp/wateros_phase_runs"
mkdir -p "$LOG_DIR"

cp "$BRINGUP" "$BACKUP"
trap 'mv "$BACKUP" "$BRINGUP"' EXIT

enable_phase() {
  local phase="$1"
  python3 - "$BRINGUP" "$phase" <<'PY'
import re, sys
path, phase = sys.argv[1], sys.argv[2]
text = open(path).read()
# 全部注释掉路径行
text = re.sub(r'^(\s*)"(/[^"]+)"', r'\1// "\2"', text, flags=re.M)
# 取消目标阶段的注释
markers = {
    "P1": "P1 basic",
    "P2": "P2 busybox",
    "P3": "P3 benchmark",
    "P4": "P4 网络",
    "P5": "P5 libctest",
    "P6": "P6 LTP",
}
start = markers[phase]
lines = text.splitlines(True)
in_phase = False
out = []
for ln in lines:
    if "--- " + start in ln:
        in_phase = True
    elif in_phase and ln.strip().startswith("// --- P") and start not in ln:
        in_phase = False
    if in_phase and re.match(r'\s*//\s*"/', ln):
        ln = re.sub(r'//\s*"', '"', ln, count=1)
    out.append(ln)
open(path, "w").write("".join(out))
PY
}

run_phase() {
  local phase="$1"
  local log="$LOG_DIR/${phase}.log"
  info "开始运行测试阶段 phase=${phase} log=${log}"
  enable_phase "$phase"
  if ! (cd "$ROOT" && make rv_qemu_run >"$log" 2>&1); then
    warning "QEMU 非正常退出 phase=${phase} action=检查_panic_与脚本日志"
  fi
  if [[ -f "$PARSER" ]]; then
    python3 "$PARSER" "$log" || true
  fi
  if grep -q "PANIC\|Panicked" "$log"; then
    error_text="内核 panic 已检出 phase=${phase} action=停止本阶段后续测试"
    warning "$error_text"
    grep -oE 'unsupported: unknown nr=[0-9]+|Panicked at [^ ]+' "$log" | head -3 | sed 's/^/    /'
  fi
  info "测试阶段结束 phase=${phase}"
}

for p in P1 P2 P3 P4 P5 P6; do
  run_phase "$p"
done

info "全部测试阶段结束 log_dir=${LOG_DIR}"
