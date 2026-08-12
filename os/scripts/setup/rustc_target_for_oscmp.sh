#!/bin/sh
# 查询当前 rustc 是否列出比赛所需的 LoongArch64 与 RISC-V64 target。

echo "loongarch64平台：\r\n\r\n"
rustc --print target-list | grep loongarch64

echo "\r\n\r\n"
echo "risc-v平台：\r\n\r\n"
rustc --print target-list | grep riscv64gc
