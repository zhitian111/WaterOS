#!/usr/bin/env bash
# 将系统时间设置为指定值（需要 root）
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=config.sh
source "${SCRIPT_DIR}/config.sh"

if [[ $# -lt 1 ]]; then
  echo "用法: sudo $0 <时间>" >&2
  exit 1
fi

TARGET_TIME="$1"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "错误: 需要 root 权限。请使用: sudo $0 '$TARGET_TIME'" >&2
  exit 1
fi

timedatectl set-ntp false 2>/dev/null || true

# timedatectl 接受 "YYYY-MM-DD HH:MM:SS"；ISO 带 T 时转换
if [[ "$TARGET_TIME" == *T* ]]; then
  # 2026-04-05T15:00:00+08:00 -> 2026-04-05 15:00:00
  normalized="${TARGET_TIME/T/ }"
  normalized="${normalized%%+*}"
  normalized="${normalized%%Z*}"
  timedatectl set-time "$normalized" || date -s "$normalized"
else
  timedatectl set-time "$TARGET_TIME" || date -s "$TARGET_TIME"
fi

hwclock --systohc 2>/dev/null || true
echo "[*] 当前系统时间: $(date)"
