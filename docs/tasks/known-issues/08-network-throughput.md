# K-08：网络栈吞吐、阻塞与锁竞争

## 任务目标

在 K-04 证明网络是 Top 3 后，提高 iperf/netperf 吞吐和请求延迟，同时保持 CAgent、
TCP/UDP 语义、virtio-net 外部网络和 loopback 一致。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-driver.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/tasks/perf/wave2-network-throughput.md`
- `docs/tasks/run_testsuits_qemu.md`
- `docs/todo/perf-baseline-gap-report.md`

## 已知信息与代码证据

旧报告中的 2 KiB 单帧缓冲已经过期。当前 smoltcp adapter 使用：

```rust
const RX_BUF: usize = 64 * 1024;
const TX_BUF: usize = 64 * 1024;
const MAX_RX_DRAIN: usize = 32;
```

并已有 `rx_staging` 批量 drain。仍可确认整个协议栈由单个全局 mutex 保护：

```rust
static NETWORK_STACK: Mutex<Option<NetworkStack>> = Mutex::new(None);
```

syscall 阻塞路径仍可能每 tick poll/sleep，send/recv/accept/connect 的 poll owner 和
全局锁竞争需要当前计数证明。CAgent 三轮 10/10 是正确性基线，不能为吞吐改变其
connect/accept/backlog 行为。

## 涉及文件

- `os/components/wateros-driver/driver-network/src/lib.rs`
- `os/components/wateros-driver/driver-network/network-impl/impl-smoltcp/src/lib.rs`
- `os/components/wateros-driver/driver-impl/impl-qemu-{riscv64-opensbi,loongarch64-virt}/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/`
- `os/src/main.rs`
- `docs/tasks/perf/wave2-network-throughput.md`
- `docs/tasks/run_testsuits_qemu.md`

## 任务内容

以下子项可并行测量，实施时每项独立提交：

1. 记录 stack mutex 获取/等待、每次 poll 处理帧数、空 poll、TX/RX 字节、socket
   buffer occupancy 和阻塞轮数。
2. 明确唯一 poll ownership：background poller 与 syscall 可以触发推进，但不得在
   同一逻辑操作重复无效 poll；修改前先证明重复。
3. 批量 RX/TX 设置公平上限，防止网络流量长期占有全局锁或饿死 timer/task。
4. 依据实测调整 TCP/UDP/socket buffer；`SO_SNDBUF/SO_RCVBUF` 的可见值和实际容量
   必须一致或有明确 Linux 兼容策略。
5. 减少 syscall staging `Vec` 和复制时，遵守 RIO-02 user-copy 及 RIO-06
   非破坏性读取契约，不能在持 stack spin mutex 时触发用户缺页或睡眠。
6. 若全局锁确为瓶颈，先缩短临界区和移出 user-copy/wait，再评估拆分 device、
   interface、socket metadata 锁；不得直接细粒度重构整个 smoltcp ownership。
7. 分别测试 loopback、virtio-net user backend 和真实外部连接；QEMU 参数必须包含
   正确的 `netdev`，不能把仅系统内 loopback 当作联网成功。

## 如何验收

- [ ] `make rv_check && make la_check` 通过。
- [ ] 修改前后三轮 iperf TCP/UDP 与 netperf STREAM/RR，报告中位数和波动。
- [ ] poll/锁/帧计数证明优化命中了预期成本，空 poll 或锁等待下降。
- [ ] CAgent 连续三轮 10/10，无 connect、accept、SIGHUP 或脚本等待回归。
- [ ] LTP socket/poll/epoll、nonblocking、timeout、shutdown 和 close 竞态通过。
- [ ] 外部网络通过 virtio-net 可达，loopback 仍正确；两种路径不混淆。
- [ ] 无持 network spin mutex 的 user-copy、sleep、scheduler wait 或日志输出。
- [ ] UDP packet 边界、TCP stream 顺序和 EFAULT 后消费语义满足 RIO-06。

结果写入 `docs/tasks/known-issues/results/k08-<subtask>-YYYYMMDD.md`。
