# wateros-platform 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时必须同时检查 `wateros-platform/Cargo.toml`、`platform-impl`、`platform-arch`、`platform-firmware` 的 feature 传递链。实现中通常需要定义 BootArgs、时间能力和固件调用桥接。

新增 arch impl 时，应优先接入 `platform-arch` 的 `api-v0` 契约，并同步检查任务系统是否仍直接依赖某个具体 ISA。当前 `platform-arch` 的 active impl 通过 `impl-riscv64` 或 `impl-loongarch64` feature 选择；具体实现需要提供时间读取、全局/时钟中断控制、任务切换上下文、trap frame 语义读写，以及 `__switch`、`__arch_task_entry`、`__arch_user_task_entry`、`__wateros_arch_restore_user_task` 等任务机制符号。

`impl-loongarch64` 当前是 API-first bring-up 实现：它补齐 LoongArch64 arch 层 crate、feature 接线和汇编骨架，但不代表已经存在完整的 qemu-loongarch64 平台、链接脚本、固件或驱动路径。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
