#!/usr/bin/env bash
# 最小验收：仅跑 glibc iozone（无 timeout），与 rv_qemu_run 基线口径一致。
set -euo pipefail
cd "$(dirname "$0")/.."
OS_DIR="$PWD"
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
    echo "IOZONE_MINIMAL: OK (iozone test complete found)"
else
    echo "IOZONE_MINIMAL: FAIL (no iozone test complete)" >&2
    exit 1
fi
if grep -qiE 'Kernel panic|RefCell already borrowed' "$LOG"; then
    echo "IOZONE_MINIMAL: PANIC detected" >&2
    exit 1
fi
