# 40 Loongson 2K1000 早期串口探针

## 任务内容

第二轮 `bootm` 已不再出现 U-Boot `CPU0 exception`，但串口也没有
WaterOS banner。为区分“U-Boot 未跳转 / 汇编入口未执行 / Rust 入口
未执行 / 后续初始化挂死”，增加两级早期探针：

1. `_start.S` 进入后立即向 UART0 写一个 `W`
2. 2K1000 的 `wateros_kernel_main` 进入后立即写
   `[2K1000] enter WaterOS Rust`

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`
- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新 `kernel-la2k.ui` 已更新到 TFTP
- [ ] 板端输出出现 `W` 或 `[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- 诊断探针已编译；等待板端串口输出。
