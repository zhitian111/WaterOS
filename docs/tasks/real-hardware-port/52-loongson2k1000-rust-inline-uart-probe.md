# 52 Loongson 2K1000 Rust 内联 UART 探针

## 任务内容

`WRS` 说明汇编 trampoline 已跳 Rust，但 Rust 首条 console 调用未输出。
为区分“未进入 Rust”和“进入 Rust 但 console 调用失败”，在
`wateros_kernel_main_rust` 首部用内联汇编直接写 UART0 字符 `T`。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出是否包含 `WRST`

## 任务简报

- 完成日期：2026-08-16
- Rust 入口内联 UART 探针已加入；等待板端串口。
