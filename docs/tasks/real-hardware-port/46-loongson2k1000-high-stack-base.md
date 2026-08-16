# 46 Loongson 2K1000 明确构造高地址 boot stack

## 任务内容

`WRS` 说明 trampoline 已进入，但 Rust 入口仍未输出。检查汇编后发现
`la.global` 对本地符号仍展开为 32 位 PC 相对寻址；在 U-Boot 高
cached 段执行时 boot stack 可能落在错误地址。

本任务让 `__wateros_arch_boot` 明确：

1. 用 `la.abs` 取 link-time 低地址
2. 从 `__wateros_kernel_high_base` 读平台高基址
3. `or` 得到运行时高 cached 栈地址

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/asm/boot.S`
- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 反汇编确认 `__wateros_arch_boot_stacks` 经 high-base 构造
- [x] 新内核已更新到 TFTP
- [ ] 板端输出继续到 `[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- 高地址 boot stack 修正；等待板端串口。
