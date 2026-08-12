#!/bin/sh
# 为当前 Rust toolchain 安装 WaterOS 使用的两个裸机编译 target。

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WOS_LOG_COMPONENT=SETUP
export WOS_LOG_COMPONENT
. "$SCRIPT_DIR/../source/console.bash"

info "开始安装 Rust 编译目标 target=riscv64gc-unknown-none-elf"
rustup target add riscv64gc-unknown-none-elf
info "Rust 编译目标安装完成 target=riscv64gc-unknown-none-elf"

info "开始安装 Rust 编译目标 target=loongarch64-unknown-none"
rustup target add loongarch64-unknown-none
info "Rust 编译目标安装完成 target=loongarch64-unknown-none"
