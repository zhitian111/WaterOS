# 48 Loongson 2K1000 高基址改为函数返回

## 任务内容

改用数据变量 `__wateros_kernel_high_base` 后，读取该变量本身需要一次
低地址数据访问，仍可能落入错误地址窗口。改为平台提供弱函数
`__wateros_get_kernel_high_base`，直接在寄存器中返回高基址，避免
boot stack 建立前访问 `.data`。

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/asm/boot.S`
- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出继续到 `WRS[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- 高基址函数化完成；等待板端串口。
