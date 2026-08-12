#!/bin/bash
# 安装 nightly 的双架构裸机 target，并为 os/ 设置 rustup override。

# 获取脚本的绝对路径
SCRIPT_DIR=$(dirname "$(readlink -f "$0")")
WOS_LOG_COMPONENT=SETUP

source "$SCRIPT_DIR/../source/console.bash"

info "开始安装 Rust 裸机 targets toolchain=nightly"
rustup target add riscv64gc-unknown-none-elf --toolchain nightly
rustup target add loongarch64-unknown-none --toolchain nightly
rustup update nightly
info "Rust 裸机 targets 安装完成"
info "开始设置 rustup override toolchain=nightly"
rustup override set nightly
info "rustup override 设置完成"
info "输出当前 Rust 工具链信息"
rustup show
