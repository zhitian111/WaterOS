# lmbench syscall 热路径阶段结果

## 结论

`lmbench-glibc` 的首项 `lat_syscall null` 不是已确认的死锁。定向诊断表明，
`benchmp` 父子进程能通过 `pselect6` 观察控制 pipe 的就绪状态，扫描和状态转换均在
继续。问题表现为执行效率异常低：即使限制为 `-N 1`，RISC-V 8 核 QEMU 在 60 秒内
仍未输出结果。

本阶段完成两项低风险热路径收敛，但尚不能宣称 lmbench 性能问题已修复：

- `getpid/getppid` 不再依次构造 task/process 完整快照；task 层在一次进程表临界区内
  直接返回 `(pid, parent_pid)`，保留原有孤儿进程和 `ESRCH` 语义。
- `poll/select` 仅在监控集合实际包含 inet socket 时驱动网络栈；pipe、普通文件和设备
  fd 不再无条件进入网络轮询。`ScanCtx` 中重复的一次网络轮询也已删除。

## 定向诊断

测试命令：

```text
cd /glibc && ./lmbench_all lat_syscall -P 1 -N 1 null
```

- 基线内核：40 秒宿主超时，未完成。
- 窄进程标识接口：40 秒超时，未完成。
- 窄接口加按需网络轮询：20 秒及 60 秒超时，未完成。
- 临时诊断确认 `pselect6(nfds=4)` 父进程与监控 fd 5 的子进程均持续扫描；pipe
  readiness 曾从 0 变为 `POLLIN`，因此不是固定卡在第一次 `pselect6`。
- 后续源码、镜像二进制反汇编及修复后复验确认，此 lmbench 版本会读取 `ENOUGH`。
  早期设置 `ENOUGH=5000` 仍超时，是当时尚未修复的 poll deadline 递归 scheduler
  锁死所致，不应归因于 `get_enough()` 忽略环境变量。所有临时日志均已删除。

## 验证

- RISC-V64 release `cargo check`：通过。
- LoongArch64 release `cargo check`：通过。
- `impl-core` host 单测因 `wateros-platform-arch` 未选择 `ArchPagingImpl` 无法编译；
  新增 registry 回归用例已由双架构 check 覆盖编译。
- QEMU 使用 `sdcard-rv.img` 的独立 qcow2 overlay，8 CPU，最长单次 60 秒。
- 未运行完整 pre/final；按约定留待夜间明确授权。

## 后续

优先检查 `gettimeofday` 在 lmbench 子进程中的返回值、时间增量和用户复制结果。若时间
正常递增，再统计一次测量区间内的 syscall/trap 数量，定位 trap 返回、信号检查或调度
记账中的固定成本。完整 pre/final 仅用于最终门禁，不应替代这些短路径诊断。
