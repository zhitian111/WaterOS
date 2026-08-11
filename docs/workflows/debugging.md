# 卡死与异常调试流程

适用于 QEMU 下的卡死、无进度、疑似死锁和异常 trap。详细命令及故障注入模式见
[`../tools/debugging.md`](../tools/debugging.md)。

## 1. 固定复现条件

记录架构（`rv` 或 `la`）、profile、SMP 数、镜像、workload、首个异常日志和提交号。
先用不写回基础镜像的入口复现：

```bash
cd os
make debug ARCH=rv PROFILE=final SMP=8
```

需要手动控制时，在终端一启动服务：

```bash
make debug-server ARCH=rv PROFILE=final SMP=8
```

在终端二连接并采样：

```bash
make gdb
make snapshot
make watch
```

`ARCH=la` 可替换为 LoongArch；只有需要保留 guest 写入时才传 `WRITE_DISK=1`。

## 2. 捕获可复现现场

不要仅凭一次“无 syscall 进度”判断死锁。确认多个采样周期内 PC、事件序列、timer 或
context switch 都未推进后，保持 QEMU 暂停并保存 `os/debug-reports/<timestamp>-.../`。

至少保留：

- `summary.txt` 与 `snapshot.json`：各 CPU 的 PC/SP/任务状态；
- `events.json`：最近 trap、syscall、IPI、futex 和锁事件；
- `gdb.txt` 与 `serial-tail.txt`：寄存器、栈和串口末尾；
- `metadata.json`：ELF、build ID 与 git 版本，确保符号匹配。

## 3. 定位与验证

1. 用 `wos-cpus`、`wos-tasks`、`wos-events`、`wos-locks` 对比所有 CPU，而非只看当前 hart。
2. 将停滞点按“锁等待、调度/中断、页表/用户拷贝、设备 I/O、用户态计算”分类；记录首个
   不再推进的事件。
3. 从 trap/syscall/调度入口沿实际调用链定位状态所有者，检查锁顺序、跨 CPU 唤醒和资源生命周期。
4. 最小修复后，以相同架构、SMP、镜像和 workload 复跑；至少验证不再停滞且原有功能未回归。

报告应链接到现场目录，并说明复现条件、首个异常、根因、修复提交和复验结果。
