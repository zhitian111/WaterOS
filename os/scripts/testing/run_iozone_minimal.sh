#!/usr/bin/env bash
# 最小验收：仅跑 glibc iozone（无 timeout），与 rv_qemu_run 基线口径一致。
# 脚本会临时替换 BRINGUP_COMMANDS，并在退出时恢复源文件。
set -euo pipefail
cd "$(dirname "$0")/../.."
OS_DIR="$PWD"
WOS_LOG_COMPONENT=TEST
# shellcheck source=/dev/null
source "$OS_DIR/scripts/source/console.bash"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    cat <<EOF
用法: ${0##*/} [LOG_FILE]

临时将 bring-up 队列替换为 glibc iozone，构建并运行 RISC-V64 内核。
LOG_FILE 默认为 /tmp/wave1_blockcache_iozone.log。
脚本会修改 src/user_bringup_busybox.rs，并在退出时从备份恢复。
EOF
    exit 0
fi
BRINGUP="$OS_DIR/src/user_bringup_busybox.rs"
BACKUP="$BRINGUP.bak.iozone_minimal"
LOG="${1:-/tmp/wave1_blockcache_iozone.log}"

cp "$BRINGUP" "$BACKUP"
restore() { cp "$BACKUP" "$BRINGUP"; }
trap restore EXIT

python3 - "$BRINGUP" <<'PY'
import sys
path = sys.argv[1]
body = open(path).read()
snippet = '''const BRINGUP_COMMANDS : &[BringupCommand] = &[
    BringupCommand { program : "/glibc/busybox", argv : &["sh", "/glibc/iozone_testcode.sh"] },
];'''
marker = '#[cfg(all(not(feature = "bringup-ltp-glibc-only"),'
idx = body.index(marker)
start = body.index('const BRINGUP_COMMANDS', idx)
end = body.index('];', start) + 2
open(path, 'w').write(body[:start] + snippet + body[end:])
PY

make kernel-rv
make rv_qemu_run 2>&1 | tee "$LOG"

if grep -q 'iozone test complete' "$LOG"; then
    info "iozone 最小验收通过 marker=iozone_test_complete"
else
    error "iozone 最小验收失败 reason=completion_marker_missing log=${LOG}" 1
fi
if grep -qiE 'Kernel panic|RefCell already borrowed' "$LOG"; then
    error "iozone 最小验收失败 reason=kernel_panic log=${LOG}" 1
fi
