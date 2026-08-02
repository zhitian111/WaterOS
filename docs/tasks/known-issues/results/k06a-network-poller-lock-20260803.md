# K06a 网络轮询锁停滞修复报告

## 问题与证据

RISC-V64 8 hart 决赛运行在 `cagent-glibc` 启动 HTTP 并发请求后停止输出，QEMU
仍持续占用约 6.3 个宿主 CPU。GDB 采样显示：

- 5 个 hart 在 `driver::network::stack::create_tcp_socket()` 获取
  `NETWORK_STACK` 时自旋；
- 2 个 hart在 `poll_at_millis()` 获取同一把锁时自旋；
- 上述 syscall 上下文的 `sstatus.SIE=0`，无法靠本核 timer 调度让出 CPU；
- 剩余 hart 位于 idle，锁持有任务没有重新获得执行机会。

`network_poller_task` 是可被 timer 抢占的内核任务，却直接获取跨 CPU 自旋锁。
若它在持有 `NETWORK_STACK` 时被切走，关中断的 syscall 调用方会永久自旋。

## 修复

涉及文件：`os/src/main.rs`。

后台网络轮询在读取当前中断状态后关闭本核全局中断，完整执行
`poll_at_millis()` 和 `poll_socket_events()`，释放协议栈锁后恢复原中断状态，
最后才进入 `sleep_for_ticks(1)`。修复没有改变 driver API、socket 语义或锁顺序。

## 验证

- `make check`：通过，仅有原有 warning。
- `make kernel-rv-final-log`：通过。
- 主办方 RISC-V 新镜像、QEMU 8 hart、8 GiB：CAgent 10/10 通过，耗时
  `3.573874900s`。
- 同轮 BuildStorm 已通过 `BUILDSTORM_TOOLCHAIN`、`BUILDSTORM_MINIBUILD`，并进入
  `tg-xtask` 预构建；原网络栈全局锁停滞未复现。
- 测试前 `e2fsck -fn os/sdcard-rv-pub.img` Pass 1 至 Pass 5 全部通过。

完整 BuildStorm 的最终结果由后续夜间报告记录，不作为本次网络锁修复的提交边界。
