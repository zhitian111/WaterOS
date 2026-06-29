# wateros-runtime — 版本概述

## 当前阶段目标

为 WaterOS 内核提供**可替换、feature 驱动**的基础运行时：在 QEMU RISC-V / LoongArch bring-up 上稳定输出 panic 与日志，并提供带中断保护的全局堆。

## 已具备的用户/开发者可见能力

- 彩色 `[WaterOS]` 前缀日志经串口/QEMU 控制台可见（级别由 `impl-warn` / `impl-error` 等 feature 控制）。
- Panic 时打印源码位置并请求平台关机。
- 内核 `alloc` / `Box` 等依赖的全局堆可用；OOM 时打印堆布局并 panic。
- 可选 ASCII 启动横幅与 virt UART 访问（伪 shell 等场景）。

## 适用范围

- **适用**：`qemu-riscv64-opensbi`、`qemu-loongarch64-virt` 等启用 `runtime/impl-platform-console` 的构建。
- **不适用**：期望静默或文件日志的生产部署（当前无持久日志——见 `wateros-klog`）。
- **注意**：`impl-dummy` 构建一旦实际打印即失败，仅用于无控制台链路的编译验证。

## 与系统其它部分的关系

runtime 是根 crate **始终依赖**的组件之一；不直接暴露 syscall。持久内核日志与用户态 `dmesg` 由 **`wateros-klog`** 承担，两者 intentionally 分离。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
