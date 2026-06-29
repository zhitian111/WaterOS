# wateros-utils — 阶段能力概述

## 当前阶段目标

预留一个**无平台依赖**的工具 crate，供后续收纳跨子系统复用的纯函数与小数据结构，而不污染 `wateros-base` 的类型边界。

## 已具备

- Crate 骨架：`#![no_std]`、独立 `cargo test` 可链接
- 占位 API `add`，验证工作区依赖解析
- 一份未接入的 RISC-V UART 调试汇编（`print_register`）

## 适用范围

- 根 `wateros` 已默认依赖，便于后续在内核中直接 `use utils::...`
- 当前阶段**无**面向应用或 syscall 的稳定工具

## 已知限制

- 公共 API 几乎为空，主线代码未使用
- 汇编调试代码未编入构建，不能作为官方调试接口
- 无 feature、无测试覆盖真实工具行为

## 下一步方向（未承诺）

- 迁入与平台无关的算法/格式化/helper
- 按需以子模块或 feature 组织 riscv 早期调试例程
- 明确与 `wateros-runtime` 日志/打印的职责边界
