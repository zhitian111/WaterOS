#!/bin/bash

# 获取脚本的绝对路径
SCRIPT_DIR=$(dirname "$(readlink -f "$0")")

source $SCRIPT_DIR/source/console.bash

info "安装工具链..."
rustup target add riscv64gc-unknown-none-elf --toolchain nightly
rustup target add loongarch64-unknown-none --toolchain nightly
rustup update nightly
info "安装工具链完成！"
info "设置项目工具链..."
rustup override set nightly
info "项目工具链设置完成！"
info "打印当前 rust 工具环境信息"
rustup show
