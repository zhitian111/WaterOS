#!/usr/bin/env bash
# 恢复 NTP 自动同步（提交完成后执行）
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "错误: 需要 root 权限。请使用: sudo $0" >&2
  exit 1
fi

echo "[*] 重新开启 NTP..."
timedatectl set-ntp true
timedatectl status
