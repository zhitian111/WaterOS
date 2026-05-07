# wateros-platform 公共 API 快照

## 当前定位

聚合 **`arch`**（ISA 原语）、**`firmware`**（SBI/控制台/定时器等）与 **`platform-impl`**（QEMU+OpenSBI 或 dummy），在上层组合 **`boot`**、**`time`**、**`timer`**、**`reset`**、**`console`**、**`interrupt`**。默认 **`impl-qemu-riscv64-opensbi`** + **`opensbi`**；**`platform-arch`** 默认 **`impl-riscv64`**，**`platform-firmware`** 默认 **`impl-opensbi`**。

其中 **`platform-arch`** 还承担任务切换上下文、trap 抽象与用户态返回等机制类型（**`ArchTaskContext`**、**`ActiveTrapFrame`** 等）；与 **task** 公共 API 的边界：task 侧只暴露架构无关快照，寄存器级细节留在 arch / scheduler / impl-core。以下表格仅列 **本聚合 `lib.rs` 直接挂出** 的模块与根级函数，深层 **`pub`** 以各子 crate 为准。

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
