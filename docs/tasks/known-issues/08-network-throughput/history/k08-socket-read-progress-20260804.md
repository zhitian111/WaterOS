# K-08 Socket `read(2)` 进展修复报告（2026-08-04）

## 问题与根因

LoongArch64 8 核 LTP `bind04`/`bind05` 在重复 IPv4 TCP/UDP 通信时曾超时。阻塞
socket 通过 VFS read lease 返回 `VfsError::Busy` 后，`acquire_read_lease()` 只执行
`task::yield_now()`。当通信双方都进入读等待时，没有一方继续 poll smoltcp，已入队的
数据无法推进到 socket，形成活锁并放大全局调度器竞争。

`wateros-debug` 的 GDB remote ABI 采样显示 CPU 6 在全局 scheduler 上产生大量
contention，且部分 CPU timer 明显落后。这帮助排除了 ext4 损坏，但采样现象本身不是
根因；最终由 VFS socket read 的 `Busy` 重试路径确认缺少网络推进。

## 修改

文件：
`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`

- 在 socket fd 获取 read lease 前调用 `drive_network_stack()`。
- socket 的 `Busy` 重试改用既有 `socket_blocking_tick()`，统一处理非阻塞返回、信号
  检查、网络推进与调度让出。
- 普通文件仍使用原有 `yield_now()`，未改变文件 offset/read lease 契约。

## 验证结果

- `make rv_check`、`make la_check`：通过（仅有既存 unused 警告）。
- RISC-V64/OpenSBI、8 CPU、musl LTP，单次启动连续运行：
  `bind04` 10 PASS、`bind05` 8 PASS；各 1 个 IPv6 TCONF；退出码均为 0，耗时
  715 ms。
- LoongArch64/QEMU、8 CPU、musl LTP，单次启动连续运行：结果同上，耗时 1.904 s。
- 两个测试镜像修改脚本后均通过 `e2fsck -fn`；QEMU 使用 snapshot 模式。

日志位于 `/tmp/wateros-bind04-bind05-{rv,la}-read-poll-fix.log`，SHA-256 分别为
`147addd38c4e44d216a13d5943506bad4b9ca01507d6a83c668d9c6734d26faa` 和
`124f1db87c45cd0d9967c71485cb986d73c30f3e5f530560f92f2beaa9026eb2`。

## 基线记录修正与剩余范围

此前 K-02B 报告将 `bind05` 描述为清理“不再前进”。后续定向 release 运行确认 LTP
会进行最多 11 次、每次约 5 秒的 SIGKILL 重试，因此不能据此认定永久卡死；可确认的
缺陷是 socket read 活锁和极慢的超时清理。本修复已消除两个定向用例的该现象。

K-08 整体仍未完成：iperf/netperf 吞吐、外部 virtio-net、nonblocking/timeout/close
竞态和 CAgent 三轮回归仍需分别验收。
