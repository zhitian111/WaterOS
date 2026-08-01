#!/bin/sh
# Run the existing LTP read-family cases with stable per-case markers.

set -u

LTP_BIN_DIR=${LTP_BIN_DIR:-/glibc/ltp/testcases/bin}
CASE_TIMEOUT=${READ_FAMILY_CASE_TIMEOUT:-45}
BUSYBOX=${READ_FAMILY_BUSYBOX:-/glibc/busybox}
DEFAULT_CASES="
open06 open09
unlink05 unlink07 unlinkat01
rename09 renameat201 renameat202
read01 read02 read03 read04 readv01 readv02 pread01 pread02 preadv01 preadv02
pipe01 pipe02 pipe03 pipe04 pipe05 pipe06 pipe07 pipe08 pipe09 pipe10 pipe11 pipe12 pipe13 pipe14
pipe2_04 socketpair01 socketpair02
recv01 recvfrom01 recvmsg01
eventfd01 eventfd02 eventfd03 eventfd04 eventfd05
eventfd2_01 eventfd2_02 eventfd2_03
"
CASES=${READ_FAMILY_CASES:-$DEFAULT_CASES}

if [ -x "$BUSYBOX" ]; then
    TIMEOUT_KIND=busybox
elif command -v timeout >/dev/null 2>&1; then
    TIMEOUT_KIND=standalone
else
    echo "READ_FAMILY infrastructure=timeout ok=false"
    exit 2
fi

run_with_timeout() {
    if [ "$TIMEOUT_KIND" = busybox ]; then
        "$BUSYBOX" timeout "$CASE_TIMEOUT" "$1"
    else
        timeout "$CASE_TIMEOUT" "$1"
    fi
}

passed=0
failed=0
missing=0

echo "READ_FAMILY_BEGIN root=$LTP_BIN_DIR timeout_s=$CASE_TIMEOUT"
for case_name in $CASES; do
    case_path="$LTP_BIN_DIR/$case_name"
    if [ ! -x "$case_path" ]; then
        echo "READ_FAMILY case=$case_name ok=false reason=missing"
        failed=$((failed + 1))
        missing=$((missing + 1))
        continue
    fi

    echo "READ_FAMILY_CASE_BEGIN case=$case_name"
    run_with_timeout "$case_path"
    rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "READ_FAMILY case=$case_name ok=true rc=0"
        passed=$((passed + 1))
    else
        echo "READ_FAMILY case=$case_name ok=false rc=$rc"
        failed=$((failed + 1))
    fi
done

echo "READ_FAMILY_RESULT passed=$passed failed=$failed missing=$missing"
[ "$failed" -eq 0 ]
