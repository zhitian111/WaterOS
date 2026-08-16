# BS-SC-03：全局 signal timer 仅由 timekeeper 到期

## 任务内容

把每 CPU 的 CPU-time accounting 与全局 realtime/POSIX timer 到期处理拆开。所有 CPU
仍保留 10ms scheduler tick；只有 timekeeper CPU 执行全局 timer 扫描。

## 实施方案

1. 将 `timer_tick(interrupted_user)` 拆为当前 CPU accounting 和全局 timer expiration。
2. 当前提交不改变 `account_cpu` 行为，避免同时引入 CPU timer hint；仅把
   `expire_realtime`/`expire_posix_timers` 限制到 scheduler 已有 timekeeper CPU。
3. 使用现有 timekeeper 判定 API；若未导出，则增加 task 内部窄 facade，不扩大 `api-v0`。
4. 生成的 signal dispatch 在 signal registry 锁外执行，保持 wake/IPI 顺序。
5. 不改变 `SCHED_TIMER_PERIOD_MS`、scheduler timeout 推进或 timer 重武装。

## 涉及文件

- `os/src/trap_handler.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`
- `os/components/wateros-task/src/schedule.rs` 或现有 timekeeper facade
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/cpu.rs`
- `history/03-brief.md`

## CodeGraph 查询

```bash
codegraph explore "timer_tick expire_realtime expire_posix_timers account_cpu"
codegraph explore "timekeeper schedule_tick timeout accounting"
codegraph callers "expire_realtime"
codegraph callers "expire_posix_timers"
```

## 验收方式

本提交属于低风险批次 A，不单独跑完整 BuildStorm。

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
cd ..
git diff --check
```

用临时诊断计数或已有 bringup stats 证明：全局 expiration 次数接近 timekeeper tick 数，
而 CPU accounting 和 scheduler tick 仍在各 CPU 发生。`setitimer`/POSIX timer 的功能与精度
在任务 04 统一回归。

## 完成后简报

新增 `history/03-brief.md`，记录两类 timer 的调用计数和最窄检查结果，并标记
“批次 A 完整验收待任务 04”。
