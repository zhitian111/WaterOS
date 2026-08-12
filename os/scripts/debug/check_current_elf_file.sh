#!/usr/bin/env bash
# 打印当前 kernel-la 与 kernel-rv 的完整 readelf 报告。
set -euo pipefail

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
OS_DIR="$(realpath "$SCRIPT_DIR/../..")"
echo "以下为kernel-la文件的elf解析信息：\r\n\r\n"
readelf -a "$OS_DIR/kernel-la"
echo "\r\n\r\n以下为kernel-rv文件符号表信息：\r\n\r\n"
readelf -a "$OS_DIR/kernel-rv"
