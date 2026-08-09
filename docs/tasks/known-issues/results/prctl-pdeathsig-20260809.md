# prctl 错误语义与 PDEATHSIG 实验记录（2026-08-09）

## 问题

`prctl02` 暴露了大量未实现 option 的错误语义；`PR_SET_PDEATHSIG` 的非法信号号
也没有校验。

## 修改

- `os/components/wateros-syscall/.../sys/task/task.rs`：
  - `PR_SET_PDEATHSIG` 校验非法信号号。
  - `PR_SET_DUMPABLE` 只接受 0/1。
  - 补齐 `PR_SET_TIMING`、no_new_privs、THP、CAP_AMBIENT、
    speculation control、securebits 的 `EINVAL/EPERM` 语义。
- `os/components/wateros-syscall/.../sys/cred/cap.rs`：`PR_CAPBSET_DROP`
  不再无权限时静默成功。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP：

- `prctl01`：`PR_SET/GET_PDEATHSIG` TPASS。
- `prctl02`：invalid option、PDEATHSIG、dumpable、timing、no_new_privs、
  THP、securebits、CAPBSET_DROP 路径全部 TPASS；seccomp/cap-ambient/
  speculation 按不支持 TCONF。

日志：`/tmp/prctl01-02-fixed2.log`。

## PDEATHSIG 投递实验（已回退）

曾尝试在 `exit_group`/最终 `exit` 路径向子进程投递 `parent_death_signal`，RISC-V
最小程序可收到 `SIGUSR2`。但 LoongArch 完整 Final 连续两轮出现 `SIGSEGV/卡死`，
与之前稳定配置不同；投递路径已回退，保留 `prctl02` 错误语义修复。后续需先在更短
进程退出负载下定位投递/中断时机，再重新合入。

## 首次回归定位（2026-08-09）

通过 reflog 与日志时间线确认：

```text
稳定提交 9c591f2d
首次引入 dc2a7d76 [fix] deliver PR_SET_PDEATHSIG on parent exit
首次失败日志 /tmp/final-after-pdeathsig-la-20260809.log
复现失败日志 /tmp/final-after-pdeathsig-la-rerun-20260809.log
复现证据 [trap] SIGSEGV signal not delivered — killing user task
随后回退 8ec13b7a [fix] keep prctl02 semantics, revert pdeathsig delivery
```

## 修复思路与实现

旧实现有两个高风险点：

1. 在退出路径中先调用 `record_current_process_exit`/`begin_current_process_exit`
   导致子进程先被托孤，再执行 `collect_child_pids` 时已经找不到原父子关系。
2. 自定义信号投递没有跳过 `ProcessState::Exiting`，可能对正在退出/重调度的子进程
   重复中断并触发后续用户态 `SIGSEGV`。

当前重新实现为更保守的版本：

- `notify_parent_death_signals(parent_pid)` 在托孤发生前调用。
- 只投递给仍为 `Running/Stopped` 等活跃状态的直接子进程；`Exited/Exiting` 跳过。
- 复用 `ensure_process_signal_state` + `send_process` + `apply_signal_dispatch`，
  只对 pending 信号调用 `interrupt_task`，不再重复 `request_task_reschedule`。
- RISC-V 静态回归连续 20 轮输出 `PDEATHSIG_OK`。

完整 LoongArch Final 尚未重跑，需在下一轮完整验收中确认不再出现
`SIGSEGV signal not delivered` 和 BuildStorm 尾部停滞。
