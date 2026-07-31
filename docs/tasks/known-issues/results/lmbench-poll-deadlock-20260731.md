# lmbench poll deadline 死锁修复结果

## 问题

`lmbench_all lat_syscall -P 1 -N 1 null` 长时间没有输出。最小复现表明，fork
子进程已收到父进程的 pipe 启动信号，但后续 syscall 无法返回用户态。

边界诊断最终定位到 `wait_current_timeout_while()`：poll 的条件闭包调用
`PollDeadline::expired()`，后者通过 `task::current_tick()` 再次获取 scheduler
全局锁。条件闭包本身已在该锁内执行，因此形成同核不可重入死锁。trap 返回路径随后
阻塞在 `restore_current_trap_frame()` 获取同一把锁。

## 修改

- scheduler 在临界区结束前发布全局原子 tick 快照。
- `current_tick()` 改为读取快照，允许 scheduler 条件闭包安全查询时间。
- 同步发布 per-CPU current-task 快照；`current_task_id()` 关闭本地中断后无锁读取，
  减少 syscall、signal 和 poll 热路径的全局锁争用。
- scheduler 仍是两个快照的唯一写入者，未改变 waitqueue、进程 registry 或调度状态机。

## 短验证

```text
task: lmbench poll deadline recursive scheduler lock
date: 2026-07-31
kernel_base_commit: 9a7f2ad2
architecture: riscv64, 8 CPUs
qemu_and_firmware: QEMU virt, OpenSBI
image: temporary clean ext4 diagnostic image
commands: 3 x 5s protocol probe; 1 x 10s lat_syscall; make check; make kernel-la
result_markers: BENCH_PROTOCOL status=0; TIME_PROBE_DONE
raw_logs: /tmp/wateros-tick-cache-{1,2,3}.log; /tmp/wateros-lat-syscall.log
```

- fork + 四 pipe 的 lmbench 风格控制协议连续 3/3 次退出 0。
- 每轮都完成子进程 raw `getppid`、结果交换、退出通知和 `waitpid`。
- 当前内核设置 `ENOUGH=1` 后，真实
  `lmbench_all lat_syscall -P 1 -N 1 null` 在约 166 ms 内退出 0，输出
  `Simple syscall: 39.0000 microseconds`。
- Linux 上以 `qemu-riscv64` 运行同一个镜像二进制：默认校准 15 秒仍未完成，
  `ENOUGH=1` 则约 22 ms 完成。默认长时间无输出主要是 lmbench
  `compute_enough()` 的用户态校准与 TCG 成本，不能据此判定 WaterOS 卡死。
- `make check` 和 `make kernel-la` 通过，只有仓库已有 warning。
- 仓库级 `cargo fmt --all -- --check` 因嵌套 workspace root 配置失败；修改文件按现有
  rustfmt 风格人工校正，`git diff --check` 通过。

## 未完成项

默认校准下的 `lat_syscall -P 1 -N 1 null` 在 10 秒短超时内仍未输出结果，但
`ENOUGH=1` 已证明完整测量协议可结束。当前仍不能宣称 lmbench 达到正式性能验收线；
完整 lmbench、pre/final 和 BuildStorm 留待夜间授权后运行。
