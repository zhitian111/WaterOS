# wateros-platform 公共 API 快照

## 当前定位

聚合 **`arch`**（ISA 原语）、**`firmware`**（SBI/控制台/定时器等）与 **`platform-impl`**（QEMU+OpenSBI 或 dummy），在上层组合 **`boot`**、**`time`**、**`timer`**、**`reset`**、**`console`**、**`interrupt`**。默认 **`impl-qemu-riscv64-opensbi`** + **`opensbi`**；**`platform-arch`** 默认 **`impl-riscv64`**，**`platform-firmware`** 默认 **`impl-opensbi`**。

<<<<<<< HEAD
其中 **`platform-arch`** 还承担任务切换上下文、trap 抽象与用户态返回等机制类型（**`ArchTaskContext`**、**`ActiveTrapFrame`** 等）；与 **task** 公共 API 的边界：task 侧只暴露架构无关快照，寄存器级细节留在 arch / scheduler / impl-core。以下表格仅列 **本聚合 `lib.rs` 直接挂出** 的模块与根级函数，深层 **`pub`** 以各子 crate 为准。
=======
其中 `platform-arch` 现在还承担了架构级任务切换上下文抽象 `ArchTaskContext` 及当前架构具体实现的组织工作，用于表达“当前 CPU 架构下任务切换最小需要保存的寄存器集合”。Stage3A 之后，arch 侧的 `goto_task_entry(...)` 语义已进一步收敛为“向任务 runtime 传递 opaque bootstrap 指针”，而不再把 task 启动协议对象暴露到公共 API。`TaskContext` 也继续只作为 `task-impl` / `task-scheduler` 的机制层细节被消费。当前 trap 抽象还额外提供了 `TrapContextRead`、`TrapContextWrite`、`ArchTrapFrame` 与 `ActiveTrapFrame`，用来把 `user_sp`、`returns_to_user`、`set_user_sp`、`set_return_to_user`、`prepare_user_return` 这类语义集中在架构层；task 机制层可直接保存当前激活架构的 trap frame，而 task 公共 API 只导出架构无关的 trap 语义快照。Trap cause 的 raw 编码解码已经从 API 层下沉到具体 arch impl，避免公共契约继续绑定 RISC-V `scause` 语义。

RISC-V arch 实现现已补出最小的 user-task 入口 trampoline 与 `__wateros_arch_restore_user_task(...)` 恢复桩，可从任务对象保存的 trap frame 直接走一次 `sret` 首次进入用户态。`platform-arch` 还新增了 `impl-loongarch64`：当前提供 LoongArch64 StableCounter 读取、全局/时钟中断开关、任务上下文、trap frame 语义读写和切换/异常返回汇编骨架；它用于验证 API-first 的 LoongArch 接入路径，不包含完整 QEMU LoongArch 平台启动链。
>>>>>>> github/main

## 事实来源

- [`os/components/wateros-platform/Cargo.toml`](../../os/components/wateros-platform/Cargo.toml)
- [`os/components/wateros-platform/src/lib.rs`](../../os/components/wateros-platform/src/lib.rs)
- **`platform-arch`** / **`platform-firmware`** / **`platform-api-v0`** 根 **`lib.rs`**

## 聚合层导出（概要）

| 项 | 说明 |
|----|------|
| **`boot`**（**`api-v0`**） | **`PlatformBootArgs`**、**`PlatformBootContext`**；**`BootArgs`** / **`BootContext`** 由 **`impl-dummy`** 或 **`impl-qemu-riscv64-opensbi`** 具体类型别名。 |
| **`pub mod arch`** | **`pub use ::arch::*`**；**`init()`** → **`arch_boot()`**。 |
| **`time`**（**`api-v0`**） | **`PlatformTime`**、**`PlatformTimeError`**、**`PlatformTimeResult`**；**`PlatformTimeImpl`** 来自 dummy 或 QEMU OpenSBI；**`frequency_hz()`**。 |
| **`timer`** | 组合 arch tick 与 firmware deadline：**`PlatformTimerError`**、**`PlatformTimerResult`**；**`now_tick`**、**`tick_hz`**、**`set_timer_deadline_tick`**、**`now_duration`**、**`set_timer_after`**（及 **`_ms`** / **`_s`**）。 |
| **`reset`** | **`pub use firmware::reset::*`**（**`reset`** / **`reboot`** / **`shutdown`** 等，以 firmware 导出为准）。 |
| **`console`** | **`pub use firmware::console::*`**（早期控制台字节/缓冲写出等）。 |
| **`interrupt`** | **`pub use arch::interrupt::*`**。 |

## 维护要求

聚合模块表、默认 impl 链或 arch/firmware 边界变化时，同步更新本文件与 **`docs/architecture/snapshot.md`** 中平台相关段落。
