# K02b 超时 Deadline 相位修复结果

## 问题

LoongArch64 8 核全量 musl LTP 中，`clock_nanosleep02`、`epoll_wait02`、
`futex_wait05`、`poll02`、`pselect01` 和 `pselect01_64` 均报告提前唤醒，
最明显的 10 ms 请求实际只等待约 7-9 ms。

## 根因与实现

syscall 层把相对时长向上取整为 10 ms 调度 tick，再用当前逻辑 tick 构造 deadline。
调用发生在一个 tick 中途时，当前 tick 已经过的时间没有计入，因此实际等待最多提前一个
tick。

修复涉及：

- `poll_engine.rs`：`PollDeadline` 改用单调时钟纳秒 deadline；每次阻塞仍复用现有
  task tick API，提前醒来时按真实剩余时间继续等待。
- `clock.rs`：`nanosleep`/`clock_nanosleep` 在单调时钟 deadline 到达前补睡剩余时间，
  并保持 `EINTR` 及 remaining time 写回语义。
- `futex.rs`：相对和绝对 futex timeout 保存对应时钟的纳秒 deadline；调度超时早于
  deadline 时重新进入 futex 条件等待。

没有修改 task scheduler 的类型、队列或公开接口。

## 验证

- `make check`：通过。
- `make kernel-la-ltp-musl`、`make kernel-rv-ltp-musl`：通过。
- LoongArch64/QEMU、8 核：上述 6 个用例全部退出 0，共 42 个计时档位通过。
- RISC-V64/OpenSBI/QEMU、8 核：同一组 6 个用例全部退出 0，共 42 个计时档位通过。
- 无 `TFAIL`、`TBROK`、panic、OOM 或超时。

日志：

- `/tmp/wateros-timeout-phase-after-v3.log`
- `/tmp/wateros-timeout-phase-rv-after.log`

作为修复前基线，本轮 LoongArch musl 全量 LTP 已完整执行 487 个用例、累计 2573 个
子测试通过，耗时约 561 秒，且无内核崩溃或死锁。
