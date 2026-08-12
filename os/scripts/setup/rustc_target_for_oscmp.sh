#!/bin/sh
# 查询当前 rustc 是否列出比赛所需的 LoongArch64 与 RISC-V64 target。

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WOS_LOG_COMPONENT=SETUP
export WOS_LOG_COMPONENT
. "$SCRIPT_DIR/../source/console.bash"

info "查询 Rust 编译目标 architecture=loongarch64"
rustc --print target-list | grep loongarch64

info "查询 Rust 编译目标 architecture=riscv64"
rustc --print target-list | grep riscv64gc
