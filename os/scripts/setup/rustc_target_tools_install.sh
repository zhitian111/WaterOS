#!/bin/sh
# 为当前 Rust toolchain 安装 WaterOS 使用的两个裸机编译 target。

echo "安装riscv64gc目标工具链\r\n"
rustup target add riscv64gc-unknown-none-elf
echo "\r\n安装riscv64gc目标工具链完成\r\n"

echo "安装loongarch64目标工具链\r\n"
rustup target add loongarch64-unknown-none
echo "\r\n安装loongarch64目标工具链完成\r\n"
