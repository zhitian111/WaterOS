# wateros-platform — 聚合层公共 API

## 用途

列出根 crate `wateros` 通过 `platform` 依赖最终使用的对外接口。impl 细节见各子 crate rustdoc 与 `docs/exports/features/wateros-platform.md`。

## 模块树（`wateros-platform/src/lib.rs`）

```text
platform::
  active_impl          # feature 选中的 platform-impl crate
  arch::*              # 再导出 wateros-platform-arch
  boot::*              # [api-v0] PlatformBoot* + BootArgs/BootContext
  time::*              # [api-v0] frequency_hz / set_frequency_hz
  timer::*             # now_tick / set_timer_after* / PlatformTimerError
  reset::*             # reboot / shutdown / reset
  console::*           # console_write_* / console_flush
  wall_clock::*        # monotonic_ns / realtime_ns / RtcTimeFields
  interrupt::*         # arch 中断控制再导出
```

## `platform::arch`

再导出 `wateros-platform-arch` 全部公开模块，并附加：

| 项 | 说明 |
|----|------|
| `arch::init()` | 调用 `arch_boot()` 安装 trap 向量 |

子模块：`time`、`task`、`trap`、`interrupt`、`paging`（见下节 arch 聚合 API）。

## `platform::time`（`api-v0`）

| 项 | 说明 |
|----|------|
| `set_frequency_hz(hz)` | 引导期写入 tick 频率（Hz） |
| `frequency_hz()` | 读缓存或回退 `PlatformTimeImpl` |
| `PlatformTime` / `PlatformTimeError` | 契约类型 |

## `platform::timer`

| 项 | 说明 |
|----|------|
| `now_tick()` | 读 arch 单调 tick |
| `tick_hz()` | 读平台频率包装为 `ArchTimeFrequency` |
| `set_timer_deadline_tick` | 编程绝对 tick deadline |
| `now_duration()` | tick + Hz → `Duration` |
| `set_timer_after` / `_ms` / `_s` | 相对时刻编程 |
| `PlatformTimerError` | `Arch` / `Platform` / `DeadlineTimer` / `NoFrequency` / `Overflow` |

## `platform::reset`

| 项 | 说明 |
|----|------|
| `reboot` / `shutdown` / `reset` | 委托 `active_impl::reset` |
| `PlatformResetType` / `Reason` / `Error` | 契约枚举 |

## `platform::console`

| 项 | 说明 |
|----|------|
| `console_write_a_byte` / `console_write_a_buffer` | 早期输出 |
| `console_flush` | 刷缓冲（若后端支持） |
| `PlatformConsoleError` | 错误类型 |

## `platform::wall_clock`

| 项 | 说明 |
|----|------|
| `monotonic_ns()` | 单调纳秒 |
| `realtime_ns()` | `CLOCK_REALTIME` 纳秒 |
| `set_realtime_ns(target)` | 设置实时钟偏移 |
| `RtcTimeFields` | Linux `rtc_time` 字段 |
| `ns_to_rtc_time` / `rtc_time_to_ns` | 互转 |

## `wateros-platform-arch` 公开面

| 模块 | 主要符号 |
|------|----------|
| `arch_boot()` | 极早 trap 向量安装 |
| `time` | `read_time_tick`、`read_time_frequency` |
| `task` | `ArchTaskContext`、`ActiveArchTaskContext` |
| `trap` | trap 帧 trait、`ActiveTrapFrame`、`prepare_user_trap_frame_access`、`timer_slice_ticks`；riscv64 另有 `set_kernel_trap_satp` |
| `interrupt` | `enable_*` / `disable_*` / `wait_for_interrupt` |
| `paging` | `active_address_space_token`、`activate_address_space_token_and_flush`、`flush_address_space_translations`、`init_paging_disable_mmu`、`enable_paging` |

## `wateros-platform-arch-api-v0` 契约摘要

| 模块 | 职责 |
|------|------|
| `trap` | `TrapFrameRead/Write`、`TrapSyscall*`、`SignalFrameCodec`、`ArchTrapFrame` |
| `time` | `ArchTime`、`ArchTimeTick`、`ArchTimeFrequency` |
| `task` | `ArchTaskContext` trait |
| `interrupt` | `ArchTimerInterruptControl` |
| `kernel_trap` | `register_kernel_trap_handler`、`invoke_kernel_trap_handler`、`wateros_kernel_trap_enter` |

## 初始化契约（根 crate 责任）

1. 选定 feature 配对 arch + platform impl（如 `impl-qemu-loongarch64-virt`）。
2. `platform::arch::init()` — 任何用户态 trap 前。
3. `arch_api::kernel_trap::register_kernel_trap_handler` — 首次 trap 前（通常紧接 `task::init`）。
4. `platform::time::set_frequency_hz` — 若已从 DTB 探测到频率，在首次 `timer` 使用前写入。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
