# wateros-syscall — 公共 API

事实来源：聚合 `src/lib.rs`（`impl-kernel` 启用时，QEMU 主线默认）。

## 启用条件

根内核 `os/Cargo.toml` 依赖 `syscall`（`wateros-syscall`）；平台 feature（`qemu-riscv64-opensbi` / `qemu-loongarch64-virt`）传递 `impl-kernel` + `abi/impl-linux-generic64`。

## 聚合层导出

| 项 | 说明 |
|----|------|
| `syscall::api` | `api-v0` 全量（`SyscallKind`、`SyscallDispatcher`、`unsupported`、`syscall_enosys_ret`） |
| `syscall::SyscallDispatcher` | trait 再导出 |
| `syscall::active_impl` | `impl-kernel` 模块别名 |
| `dispatch_syscall_from_trap(nr, args)` | trap 返回路径分发 → `isize` |
| `is_restartable_syscall(nr)` | EINTR 后是否自动重启 |
| `timer_tick(interrupted_user)` | 时钟 tick（信号/调度协作） |
| `deliver_pending_signal(frame, restart)` | 投递待处理信号 |
| `restore_signal_frame(frame)` | 恢复信号帧 |
| `raise_current_signal(sig)` | 向当前任务发信号 |
| `drop_reaped_task_runtime_resources(tid, aspace)` | reap 后清理 syscall 侧资源 |
| `record_user_page_fault_handled()` | bring-up 计数 |
| `log_thread_bringup_stats_summary()` | 输出 bring-up 统计 |
| `__wateros_syscall_dispatch_current` | C ABI / 汇编入口（6 寄存器参数） |

## api-v0 契约摘要

| 类型 | 要点 |
|------|------|
| `SyscallKind` | 与 ABI 号表解耦的语义槽位；`decode::<T>()`、`label()` |
| `SyscallDispatcher` | 每槽位 `dispatch_*` 默认 `-ENOSYS`；`dispatch_syscall_from_trap` 巨型 match |
| `unsupported` | bring-up panic 辅助（未实现槽位/未知号） |

## impl-kernel 再导出（经 `active_impl`）

内核 crate 不单独被根 `wateros` 依赖；能力经聚合门面与 `KernelSyscallDispatcher` 暴露。

## 未导出 / 需注意

- 仅 `api-v0`、无 `impl-kernel` 时聚合层无 trap 入口。
- `SyscallKind` 齐全不等于 `sys_*` 已实现；以 `impl-kernel` 为准。
- 内部 `sys::*` 为 `pub(crate)`，不跨 crate 使用。
