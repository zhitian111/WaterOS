# 性能任务：网络吞吐优化（G8 iperf/netperf）

## 任务目标

使 **iperf / netperf** 各项 score **> 1.0**（当前全部卡在 baseline 1.0）。

## 背景（必读）

- `docs/todo/perf-baseline-gap-report.md` §G8
- 因果：全局 `NETWORK_STACK` Mutex、syscall 与 `socket_send` **重复 poll**、RX 单帧 2048B、send 路径 `Vec` 拷贝

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 执行前必须参考的文档

- `docs/todo/perf-ipc-sync.md`（网络相关）
- `docs/prompts/tasks/run_testsuits_qemu.md`（P4 网络阶段）

## 需要优先查看的源文件

| 文件 | 用途 |
|------|------|
| `os/components/wateros-driver/driver-network/src/lib.rs:141,677-691` | 全局锁、socket_send 内 poll |
| `os/components/wateros-driver/driver-network/network-impl/impl-smoltcp/src/lib.rs:14-15,81-129` | RX/TX 2048、单帧 receive |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/sendto.rs`、`write.rs`、`recvfrom.rs` | 重复 poll、`Vec` 分配 |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/socket_block.rs` | `sleep_for_ticks(1)` |
| `os/src/main.rs` | 背景 poller |

## 实施要点

1. 去掉 `socket_send`/`socket_recv` **内部**多余 `poll()`，由 syscall 层或单一 poller 驱动。
2. `receive()` **drain** 多帧直至空或上限，batch 喂 smoltcp。
3. RX/TX/UDP 缓冲扩至 ≥8~64KiB；评估 `SO_RCVBUF` 是否真生效。
4. TCP bulk send：减少中间 `Vec`，`copy_from_user` 直写 smoltcp 缓冲（注意 COW）。
5. 缩短 `NETWORK_STACK` 锁持有：单次锁内 send+必要 poll。

## 验收标准

- [ ] `make rv_check && make la_check`
- [ ] P4 iperf3 + netperf（loopback）吞吐上升
- [ ] LTP socket 类抽样无回归

## 风险

- **中**：锁序、阻塞语义、UDP 丢包边界

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave2-network-throughput/task.md

请优化网络栈：去重 poll、RX batch drain、扩缓冲 8KiB+。
make rv_check && la_check，P4 跑 iperf TCP 对比。
```
