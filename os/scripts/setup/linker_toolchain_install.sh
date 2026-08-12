#!/bin/sh
# 在 Debian/Ubuntu 上安装 RISC-V 裸机 GCC 与 binutils 链接工具。

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WOS_LOG_COMPONENT=SETUP
export WOS_LOG_COMPONENT
. "$SCRIPT_DIR/../source/console.bash"

info "开始安装 RISC-V 链接工具链"
sudo apt install gcc-riscv64-unknown-elf -y
sudo apt install binutils-riscv64-unknown-elf -y
info "RISC-V 链接工具链安装完成"
