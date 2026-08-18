# 45 Loongson 2K1000 boot 地址改用 la.global

## 任务内容

`WRS` 说明汇编 trampoline 已进入，但跳入 Rust 后仍卡住。进一步确认
`la.local` 生成的 32 位 PC 相对地址在高 cached 段不可靠：

1. `__wateros_arch_boot` 计算 boot stack 改用 `la.global`
2. `_start.S` 跳转 Rust 函数改用 `la.global + jr`

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/asm/boot.S`
- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出继续到 `[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- 高地址寻址修正；等待板端串口。
