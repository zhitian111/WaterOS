# RIO-10 网络地址与可中断 poll 竞态修复

## 问题与根因

8 核 RISC-V/LoongArch LTP 回归最初只有 31 个用例通过。`recv01` 和
`recvfrom01` 在连接 `getsockname()` 返回的 `0.0.0.0` 时得到 `ECONNREFUSED`；网络
模块重构后将该地址直接传给 smoltcp，而 Linux 会将作为连接目标的 `INADDR_ANY`
视为本机地址。

修复地址后，`recvmsg01` 在 8 核下仍会卡在测试清理阶段的
`kill(server, SIGKILL) + wait(NULL)`。`gdb-debug` 内核的 stall task 快照显示父进程
阻塞于 `ChildExit`，server 子进程持续一 tick 睡眠。信号可能在子进程仍 Running、
即将进入 sleep 时到达；此时 `interrupt_task()` 尚找不到等待队列，而 poll/select
循环醒来后不检查 pending signal，导致 SIGKILL 永久滞留。

## 修复

- smoltcp 实现将 connect 目标 `0.0.0.0` 规范化为 `127.0.0.1`，并保留其它地址。
- pollfd 与 fd-set 两个阻塞循环在无 fd 就绪时逐轮检查可投递信号，关闭
  Running-to-Sleep 丢失唤醒窗口并返回 `EINTR`。

涉及文件：

```text
os/components/wateros-network/network-impl/impl-smoltcp/src/stack/socket.rs
os/components/wateros-syscall/syscall-impl/impl-kernel/src/poll_engine.rs
```

## 验证

- `make rv_check`、`make la_check`：通过。
- `make kernel-rv-ltp-glibc`、`make kernel-la-ltp-glibc`：通过。
- RISC-V 8 核定向 `pipe13 + recvmsg01` 连续 10 轮：20/20 通过，无超时。
- RISC-V/LoongArch 8 核完整 runner：两边均为 34 个现存用例全部通过；12 个
  missing 用例由既有 root-layout 不适配清单删除，未恢复过滤项。
- 两个 qcow2 均通过 `qemu-img check`；合并后 `e2fsck -fn` 五阶段通过。

日志及 SHA-256：

```text
/tmp/wateros-rio10-rv-finalcheck.log       2c105725...b5730fb
/tmp/wateros-rio10-la-finalcheck.log       e5451120...afd0ce
/tmp/wateros-rio10-rv-finalcheck-fsck.log  1792c48d...0bdf0
/tmp/wateros-rio10-la-finalcheck-fsck.log  d14af195...e4527
/tmp/wateros-rio10-gdb-run.log             29814291...6ab4d
```

原始镜像未写入，哈希保持为 RISC-V `e6389737...ab98af`、LoongArch
`87ec97a2...f9dbc`。单独 host `cargo test` 因未选择 platform-arch 实现而无法构建；
目标架构构建和 QEMU 回归已覆盖新增逻辑。RIO-10 仍需完成被过滤语义的替代覆盖及
final workload 门禁，因此本报告不勾选总任务完成状态。
