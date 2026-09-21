# 44 Loongson 2K1000 Rust 入口 trampoline

## 任务内容

`WR` 说明 boot.S 已准备跳 `wateros_kernel_main`，但仍未看到 Rust
探针。为确认 `bl wateros_kernel_main` 落点是否真的到达 Rust 入口，
把 2K1000 的 Rust 函数改名为 `wateros_kernel_main_rust`，并由
`_start.S` 提供汇编 `wateros_kernel_main` trampoline：

1. 汇编 trampoline 先输出 `S`
2. 然后跳转 `wateros_kernel_main_rust`

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`
- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出包含 `WRS`；若只到 `WR` 则 `bl` 未落到 trampoline

## 任务简报

- 完成日期：2026-08-16
- 增加 Rust 入口 trampoline 探针；等待板端串口。
