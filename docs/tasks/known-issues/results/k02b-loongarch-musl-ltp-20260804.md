# K-02B LoongArch-musl LTP 复验报告（2026-08-04）

## 环境与范围

- LoongArch64/QEMU、8 CPU、1 GiB，`bringup-ltp-musl-only`。
- 使用修复过旧基础镜像既存损坏、且运行前通过 `e2fsck -fn` 的 4 GiB 测试副本。
- LTP 版本 20240524；运行镜像内原始二进制和完整遍历脚本。
- 原始日志：`/tmp/wateros-k02b-la-musl-ltp-full.log`，SHA-256
  `213906569a813e0cd2c373872720e4438f55f665d1e89242032e67c5cf7a3844`。

## 结果

旧评分中的“LoongArch-musl 整套未运行/0 分”已不可复现。完整脚本启动了 27 个用例，
其中 26 个返回，累计出现 324 个 `TPASS`、9 个 `TFAIL`、18 个 `TCONF`，证明 mount、
musl ELF、busybox runner 和基础 syscall 链均可运行。

前 26 项中非零返回为 `access02`、`acct02`、`alarm05`、`alarm06`、`asapi_01` 和
`bind04`。`bind05` 启动后在 IPv4 wildcard UDP 变体超时，测试清理不再前进，因此本轮
在保留日志后人工结束，没有伪造完整结束标记。

运行后的镜像通过 `e2fsck -fn` 全部五阶段检查：1777 个文件、270968 个块，无结构
错误。fsck 日志 SHA-256 为
`b3eb7e65e69aad2de6ec53f10d012881cee03ae8bbf0b2d63ea5e7bd00495ccc`。

## 首个阻塞根因

定向 `bind04` 复现显示 AF_UNIX、IPv4 loop TCP 等前置变体可通过，阻塞发生在重复协议
或 wildcard 地址变体。LTP 超时处理使用 `kill(-test_pid, SIGKILL)`，但连续十次仍不能
清理测试进程。

使用 `wateros-debug` 的 LoongArch GDB ABI 定向采样后，CPU 6 的任务 38 在全局
`SCHEDULER`（`0x905010c8`）产生大量 contention；CPU 0 的 timer 停在 93，而多数空闲核
推进到约 1320。PC 落在 scheduler spin、进程快照和内核堆中。现场不是 ext4 故障，
而是 socket 阻塞/进程组 SIGKILL 清理触发的调度竞争与进展问题。宿主缺少支持
LoongArch 的 `gdb-multiarch`，因此本轮通过工具内置 GDB remote 读取诊断 ABI，未生成
完整 DWARF 栈报告。

## 后续

K-02B 的“有效统计”目标完成。完整结束仍由 K-08 网络阻塞语义和 K-06C 线程退出/信号
清理继续承接；修复后应先定向重跑 `bind04`、`bind05` 和负 PID `kill`/`setpgid` LTP，
再运行 LoongArch-musl 全量脚本。K-02A 的 10,000 次 IPI/TLB 压测仍独立开放。
