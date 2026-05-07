# wateros-platform 功能快照

## 用途

记录 **`wateros-platform`** 在默认 **`impl-qemu-riscv64-opensbi`** 路径下对 **RISC-V64 架构子系统**、**OpenSBI 固件子系统**、引导参数、定时器、控制台、复位与中断的聚合导出与 feature 链。

## 事实来源

- `os/components/wateros-platform/Cargo.toml`
- `os/components/wateros-platform/src/lib.rs`
- `os/components/wateros-platform/platform-arch/`、`platform-firmware/`、`platform-impl/impl-qemu-riscv64-opensbi/`
- `os/Cargo.toml`（**`qemu-riscv64-opensbi`** → **`platform/impl-qemu-riscv64-opensbi`**）

## Feature 链（摘要）

- 根包 **default**：`api-v0`、`impl-qemu-riscv64-opensbi`。
- **`api-v0`**：转发 **`arch`**、**`firmware`**、**`impl-dummy`**、**`impl-qemu-riscv64-opensbi`** 的 api 联动。
- **`impl-qemu-riscv64-opensbi`**：启用 **`opensbi`** → **`firmware/impl-opensbi`**。
- **`platform-arch`**：**`default`** 含 **`impl-riscv64`**；**`api-v0`** 同时打开 arch 侧 dummy 与 riscv64 impl 的 api 依赖（见子 **`Cargo.toml`**）。
- **`platform-firmware`**：**`default`** 含 **`impl-opensbi`**。

## 聚合导出

- **`boot`**（**`api-v0`**）：**`BootArgs`** / **`BootContext`** 在 **`impl-dummy`** 与 **`impl-qemu-riscv64-opensbi`** 间二选一。
- **`arch`**：架构入口 **`init()`** → **`arch_boot()`**；**`trap`**、**`interrupt`**、**`paging`**、**`time`**（与 task 协作的类型）等自子 crate 再导出。
- **`time` / `timer`**：平台时间抽象与 tick、**`set_timer_after*`** 等组合 **`arch`** 与 **`firmware`**。
- **`reset` / `console`**：转发固件子系统。
- **`interrupt`**：转发 **`arch::interrupt`**。

## 真实实现 vs 占位

- **默认主线**：**`impl-riscv64`**（trap、中断、分页、时间等）+ **`impl-opensbi`**（控制台、定时器、关机/重启）+ **`impl-qemu-riscv64-opensbi`**（QEMU + OpenSBI 引导上下文与时间类型；时间频率当前为**写死常量**，注释说明可改为 DTB）。
- **`platform-impl-dummy`**：**`boot` / `time`** 为 **`unimplemented!()`** 或 **`Unsupported`**，仅应在显式选用 dummy 平台 impl 时使用。
- **`arch-impl-dummy`**：与默认 **`impl-riscv64`** 并存；不参与默认架构行为。

## 明确未覆盖

- QEMU 时间频率与 DTB 解析对齐（当前硬编码）。
- 若仅关闭 **`impl-riscv64`** 保留 arch dummy，**`paging`** 等模块当前无完整 dummy 替代路径（默认 feature 组合下不构成问题）。

## 维护要求

默认平台 impl、OpenSBI 接线或聚合导出变化时，同步更新本文件、**`docs/architecture/snapshot.md`** 与 **`docs/guides/workflow.md`**（若影响启动顺序描述）。
