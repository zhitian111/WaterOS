# K-25 `sched_getaffinity` 大 cpusetsize 修复（2026-08-06）

## 问题

RISC-V 决赛 guest 中执行 `nproc` 只返回 `1`，导致 Cargo 默认并行度错误，8 个 vCPU
大部分时间处于 idle。BusyBox 的 `nproc` 调用
`sched_getaffinity(0, sizeof(mask), mask)`，`sizeof(mask)` 为 8192 字节；内核此前把
`cpusetsize > 4096` 直接判为 `EINVAL`，因此用户态得不到 affinity mask，只能回退输出
`1`。

## 修复

按 Linux 语义处理 affinity 系统调用：

- `sched_getaffinity` 不再拒绝大的 `cpusetsize`，只向用户态写回内核实际支持的
  8 字节 CPU mask，并返回 mask 大小。
- `sched_setaffinity` 同样只从用户态读取内核实际需要的 8 字节，避免为大 cpusetsize
  分配不必要的内核缓冲。

## 验证

```text
make rv_check
make la_check
make kernel-rv-final
```

- 临时 bringup 命令 `nproc` 输出 `8`。
- 同一 Final 内核短测 CAgent 10/10，进入 BuildStorm，无 panic/deadlock。
- 完整 RISC-V BuildStorm 编译 136 个 crate 完成，`cargo xtask` 报告
  `done (1512.91s)`，即比此前 `nproc=1` 的完整日志有明显缩短。

与 `exit_group` 在用户 trap 边界强制退出 Exiting 进程的修复组合后，完整 Final 正常
输出：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1567.28 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

对应日志：

```text
/tmp/k25-full-exiting-rv-1785995120.log
```
