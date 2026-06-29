#!/usr/bin/env bash
# 通过 debugfs 向 sdcard 镜像注入「只跑指定 LTP basename」的 ltp_testcode.sh。
# 不改 test_case 源树；仅改 os/sdcard-rv.img（或指定 SDIMG）。
#
# 用法（在 os/ 目录）:
#   ./ltp_log_final/.agent/inject_ltp_whitelist.sh setpriority01 setpriority02 setpgrp02
#   ./ltp_log_final/.agent/inject_ltp_whitelist.sh --file .agent/unskip_lists/W0-A-epoll.txt
#   LIBC=musl ./ltp_log_final/.agent/inject_ltp_whitelist.sh waitpid01
#
# 环境变量:
#   SDIMG=os/sdcard-rv.img
#   LIBC=glibc|musl  (默认 glibc)
#   LTP_TIMEOUT=86400  per-case 外层 timeout 秒数（busybox timeout）

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

SDIMG="${SDIMG:-$ROOT/sdcard-rv.img}"
LIBC="${LIBC:-glibc}"
LTP_TIMEOUT="${LTP_TIMEOUT:-86400}"

if [ "$LIBC" = "glibc" ]; then
  SCRIPT="/glibc/ltp_testcode.sh"
  MARKER="ltp-glibc-whitelist"
else
  SCRIPT="/musl/ltp_testcode.sh"
  MARKER="ltp-musl-whitelist"
fi

CASES=()
if [ "${1:-}" = "--file" ]; then
  shift
  [ -n "${1:-}" ] || { echo "missing --file path" >&2; exit 2; }
  mapfile -t CASES < "$1"
  shift
fi
CASES+=("$@")
[ "${#CASES[@]}" -gt 0 ] || {
  echo "usage: $0 [--file list.txt] case1 [case2 ...]" >&2
  exit 2
}

# 去重保序
tmp_cases="$(mktemp)"
printf '%s\n' "${CASES[@]}" | awk '!seen[$0]++' >"$tmp_cases"
mapfile -t CASES < "$tmp_cases"
rm -f "$tmp_cases"

# shellcheck 数组注入到 heredoc
CASES_SHELL="$(printf '"%s" ' "${CASES[@]}")"

tmp_script="$(mktemp)"
cat >"$tmp_script" <<EOF
#!/bin/bash
echo "#### OS COMP TEST GROUP START ${MARKER} ####"
WHITELIST=(${CASES_SHELL})
target_dir="ltp/testcases/bin"
for base in "\${WHITELIST[@]}"; do
  file="\$target_dir/\$base"
  if [ ! -f "\$file" ]; then
    echo "SKIP LTP CASE \$base (missing)"
    continue
  fi
  echo "RUN LTP CASE \$base"
  timeout ${LTP_TIMEOUT} "\$file"
  ret=\$?
  echo "FAIL LTP CASE \$base : \$ret"
done
echo "#### OS COMP TEST GROUP END ${MARKER} ####"
EOF

debugfs -w -R "rm $SCRIPT" "$SDIMG" 2>/dev/null || true
debugfs -w -R "write $tmp_script $SCRIPT" "$SDIMG"
debugfs -w -R "rm /glibc/.ltp_resume_after" "$SDIMG" 2>/dev/null || true
debugfs -w -R "rm /musl/.ltp_resume_after" "$SDIMG" 2>/dev/null || true
rm -f "$tmp_script"

echo "[inject_ltp_whitelist] wrote $SCRIPT on $SDIMG"
echo "[inject_ltp_whitelist] cases (${#CASES[@]}): ${CASES[*]}"
