# K-06A Scheduler 中断守卫修复报告

```text
task: K-06A scheduler interrupt guard lifecycle
date: 2026-08-01
kernel_commit: 22a13f2eccb48022445b0e483c9c69a11e6e0065 + 本报告对应未提交修复
user_submodule_commit: 工作区已有改动，未纳入本任务
architecture: RISC-V64 8 CPU；LoongArch64 静态检查
qemu_and_firmware: QEMU 11.0.2, OpenSBI 1.7
image_sha256: e2d9467140b224786fdcdc2a4cdce77c40a8de2c1c1ea4496a60e6a19c2d2a94
overlay: /tmp/wateros-ltp-guard-fix-2.qcow2
commands: make rv_check; make la_check; musl LTP epoll-ltp; musl LTP exit_group01
result_markers: epoll_ctl 13824/13824 passed; runner completed cleanup and shutdown
first_failure: exit_group01 timeout（后续定位为同核 CFS Yield 饥饿）
raw_log_path: /tmp/wateros-ltp-guard-fix-2.log
raw_log_sha256: 临时日志，未纳入仓库
```

## 问题与修改

`InterruptGuard` 的设计注释和调用方都依赖 RAII：构造时保存并关闭全局中断，离开
调度临界区时恢复原状态。但原类型没有实现 `Drop`，因此 `suspend_current_and_run_next`
等无需上下文切换便返回的路径会遗留关闭的中断状态。

修复为 `InterruptGuard` 实现 `Drop`，统一恢复构造时保存的状态；显式 `release()`
改为消费并 drop 守卫。上下文切换路径仍在 `__switch` 返回原任务后恢复该任务保存的
状态，不改变 task API、runqueue 或锁序。

## 验证与边界

- `make rv_check`、`make la_check` 通过，仅有既存 warning。
- 初赛镜像 musl `epoll-ltp` 的 13,824 组 `fork + epoll_ctl` 全部通过。
- 压力后 runner 的单调时间继续推进到约 145 秒，并能完成失败清理和关机。
- `exit_group01` 仍失败。清理前快照显示 LTP 父任务与最后一个 Ready worker 同属 CPU0，
  父任务在 `waitpid` 的 `yield_now()` 循环中反复占用 CPU；这是独立的 CFS Yield
  进展问题，不能作为本守卫修复的通过项。

测试使用镜像内原始 LTP 二进制，并按
`test_case/ltp-full-20240524/testcases/kernel/syscalls/exit_group/exit_group01.c` 核对语义。
临时 bring-up 命令和任务快照代码均已移除。
