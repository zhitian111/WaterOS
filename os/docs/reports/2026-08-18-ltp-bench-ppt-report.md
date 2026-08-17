# WaterOS RISC-V 测试与性能数据报告

> PPT 展示版数据底稿｜数据源：`os/output.log`｜整理日期：2026-08-18

## 1. 一页结论

- WaterOS `rv/pre` 在 8 核、8 GiB QEMU 环境中连续完成 cyclictest、LTP、libcbench、lmbench 与 iozone 测试队列，五个顶层命令均返回 `exit_code=0`。
- 完整队列耗时约 12 分 28 秒；其中 LTP 运行 341 个 case，耗时 335.398 秒。
- LTP 日志中可确认 19 条 `TFAIL`，分布于 11 个 case。其余失败数不能从现有 wrapper 文本可靠推导，不能把顶层退出码 0 表述为“LTP 全部通过”。
- lmbench 测得简单系统调用 30.81 μs、pipe latency 341.68 μs、fork+exit 1.66 ms、pipe bandwidth 330.52 MB/s。
- iozone 四进程吞吐测试中，顺序读（parent 口径）30.98 MiB/s、`pread` 33.60 MiB/s、`pwrite` 11.83 MiB/s；同步写路径仍存在 `fsync: EIO`，自动模式因此中断。
- 原日志还包含 7,395 次共享文件映射销毁写回 `AccessViolation`，属于内核 VMA/权限生命周期问题，不应作为性能结果忽略。

## 2. 测试环境与口径

| 项目 | 配置 |
|---|---|
| WaterOS 版本 | `v0.1.0-prototype.1181+main` |
| 架构 / Profile | RISC-V 64 / `pre` |
| CPU | QEMU virt，8 HART，`SMP=8` |
| 内存 | QEMU 8 GiB（本次为扩容后的诊断配置） |
| 运行模式 | `MODE=auto`，snapshot |
| 用户态组合 | LTP-musl；cyclictest/libcbench/lmbench/iozone-glibc |
| 原始日志 | `os/output.log`，17,971 行 |

说明：8 GiB 不是此前 1 GiB 配置的同口径对比数据；本报告适合展示“当前功能覆盖与单次性能”，不适合直接声称扩容前后的性能提升。

## 3. 总体执行情况

| 测试组 | 耗时 | 顶层退出码 | 结论 |
|---|---:|---:|---|
| cyclictest + hackbench | 23.229 s | 0 | 四组命令完成；压力下 P8 有 6 个线程采样数为 0 |
| LTP musl | 335.398 s | 0 | 341 case 被调用；存在 19 条明确 `TFAIL` |
| libcbench glibc | 9.588 s | 0 | 全部列出的 micro-benchmark 输出完成 |
| lmbench glibc | 254.117 s | 0 | 核心 latency/bandwidth 有结果；shell 子项路径配置异常 |
| iozone glibc | 124.092 s | 0 | 多进程专项完成；自动模式被 fsync EIO 中断 |
| **总计** | **约 748.5 s** | **队列完成** | **无内核 panic / heap OOM** |

## 4. LTP 结果

### 4.1 可确认的覆盖范围

- 调用 case：341 个。
- 产生标准 `Summary:` 区块：287 个。
- 顶层 LTP 脚本耗时：335.398 秒，最终退出码 0。
- 明确 `TFAIL`：19 条，涉及 11 个 case。

现有脚本对每个 case 都输出 `FAIL LTP CASE ...`，且其尾部数字会被前序状态影响，因此该行不能用来统计通过率。PPT 中建议只展示“341 个 case 被执行、11 个 case 有明确断言失败”，不要展示未经修正的百分比。

### 4.2 明确失败项

| 分类 | Case | TFAIL 数 | 现象 |
|---|---|---:|---|
| 记账 | `acct02` | 1 | acct 文件为空 |
| 时间 / 调度 | `clock_nanosleep02` | 2 | 睡眠时间过长 |
| Futex / 时间 | `futex_wait05` | 2 | futex wait 时间过长 |
| UTS | `gethostname02` | 1 | 长度不足时错误地成功 |
| 进程 | `getpgid01` | 1 | `getpgid(1)` 返回 ESRCH |
| 内存锁定 | `mlockall02` | 1 | errno 不符合预期 |
| VFS | `pathconf02` | 5 | 非目录、空路径、超长路径、权限、符号链接错误未正确返回 |
| VFS | `readlink03` | 2 | 非预期成功；期望 ENOENT 却得到 EBADF |
| 堆管理 | `sbrk01` | 2 | `sbrk(+/-8192)` 返回 ENOMEM |
| 优先级 | `setpriority02` | 1 | 期望 EPERM，却返回 ESRCH |
| 时间权限 | `settimeofday02` | 1 | 期望 EINVAL，却返回 EPERM |
| **合计** | **11 个 case** | **19** |  |

## 5. 实时性：cyclictest

单位：日志原始数值（该环境提示“不支持高精度定时器”，绝对值仅供同配置比较）。

| 场景 | 线程 | 样本数 C | Min | Avg | Max |
|---|---:|---:|---:|---:|---:|
| 无压力 P1 | T0 | 101 | 100 | 9,390 | 18,289 |
| hackbench 压力 P1 | T0 | 89 | 121 | 10,871 | 21,450 |
| 无压力 P8 | T0–T7 | 85–104 | 97–379 | 9,249–10,429 | 19,985–22,286 |
| hackbench 压力 P8 | T0 | 93 | 71 | 10,359 | 20,976 |
| hackbench 压力 P8 | T3 | 26 | 97 | 10,206 | 15,023 |
| hackbench 压力 P8 | T1/T2/T4–T7 | 0 | — | — | — |

展示结论：单线程压力场景平均延迟较无压力场景约增加 15.8%，最大值约增加 17.3%；P8 压力结果因多数线程没有有效样本，不能作为 8 核实时性结论。

## 6. libcbench

### 6.1 内存分配

| 项目 | 时间（s） | 峰值 resident（日志单位） |
|---|---:|---:|
| malloc sparse | 0.097777 | 39,676 |
| malloc bubble | 0.047124 | 40,048 |
| malloc tiny1 / tiny2 | 0.007188 / 0.005957 | 1,372 / 1,372 |
| malloc big1 / big2 | 0.073079 / 0.061672 | 848 / 80,876 |
| malloc thread stress | 0.086912 | 984 |
| malloc thread local | 0.046654 | 1,004 |

### 6.2 线程、字符串与 stdio

| 项目 | 时间（s） |
|---|---:|
| pthread create/join serial1 | 1.903438 |
| pthread create/join serial2 | 2.692004 |
| pthread create serial1 | 1.488424 |
| pthread useless lock | 0.111299 |
| stdio putc/getc | 0.770624 |
| stdio putc/getc unlocked | 0.724744 |
| memset | 0.012342 |
| strchr | 0.015866 |
| strlen | 0.016440 |
| regex compile | 0.032829 |
| regex search（简单） | 0.009584 |
| regex search（`a{25}b`） | 0.190194 |

## 7. lmbench

### 7.1 延迟

| 项目 | 结果 |
|---|---:|
| Simple syscall | 30.8128 μs |
| Simple read | 49.1297 μs |
| Simple write | 65.6792 μs |
| Simple fstat | 54.3141 μs |
| Simple stat | 208.3884 μs |
| Simple open/close | 391.4311 μs |
| Select on 100 fd | 471.5581 μs |
| Signal handler installation | 38.5172 μs |
| Signal handler overhead | 265.9057 μs |
| Protection fault | 228.6944 μs |
| Pipe latency | 341.6813 μs |
| Process fork+exit | 1,660.0000 μs |
| Process fork+execve | 1,758.6667 μs |
| Process fork+`/bin/sh -c` | 93,012 μs* |

\* shell 子项同时出现 13 次 `/code/lmbench_src/bin/build/lmbench_all: not found`，该值应标为“待修正脚本后复测”。

### 7.2 带宽

| 项目 | 结果 |
|---|---:|
| Pipe bandwidth | 330.52 MB/s |
| `/var/tmp/XXX` write bandwidth | 11,868 KB/s* |
| `/var/tmp/XXX` page fault | 67.5117 μs |

\* 同期出现 `fsync fd=3 flush failed: Io`，落盘语义不完整。

## 8. iozone（4 进程吞吐）

共同参数：4 个进程、record size 1 KiB、file size 1 MiB。表中采用 parent 观察口径，更接近整体端到端吞吐。

| 模式 | Parent throughput（KB/s） | 约 MiB/s |
|---|---:|---:|
| initial write | 2,635.54 | 2.57 |
| rewrite | 5,203.37 | 5.08 |
| sequential read | 31,721.59 | 30.98 |
| reread | 31,431.82 | 30.70 |
| random read | 23,012.84 | 22.47 |
| random write | 4,684.69 | 4.57 |
| reverse read | 23,406.24 | 22.86 |
| stride read | 23,771.85 | 23.21 |
| fwrite | 2,723.16 | 2.66 |
| fread | 32,712.53 | 31.95 |
| pwrite | 12,116.83 | 11.83 |
| pread | 34,409.60 | 33.60 |
| pwritev（脚本输出名为 initial write） | 2,503.77 | 2.45 |
| preadv（脚本输出名为 rewrite） | 5,136.71 | 5.02 |

补充：`./iozone -a -r 1k -s 4m` 自动模式在第一行数据时因 `fsync: Input/output error` 中断；随后七组专项吞吐测试均打印 `iozone test complete`。日志共记录 58 次 fsync flush `Io` 告警。

## 9. 稳定性与异常观察

| 观察项 | 次数 | 影响 |
|---|---:|---|
| 地址空间销毁共享文件写回 `AccessViolation` | 7,395 | 大量短进程退出时重复写回失败，污染日志并暴露 VMA 权限/生命周期缺陷 |
| trap probe（多为 VA `0x10000000` lazy mapping） | 2,617 | lmbench 日志噪声显著，影响性能日志可读性 |
| fsync flush `Io` | 58 | iozone 自动模式中断，写性能结果不能等同于可靠持久化性能 |
| 内核 heap OOM / panic | 0 | 本次 8 GiB pre 队列未复现 |

## 10. 建议的 PPT 页面结构

1. **测试全景**：环境、5 类测试、总耗时 12 分 28 秒。
2. **功能兼容性**：LTP 341 case 覆盖；11 个明确失败 case 按子系统归类。
3. **实时性**：无压力与 hackbench 压力下 P1 的 Avg/Max 对比；明确 P8 样本缺失。
4. **系统调用与进程性能**：lmbench syscall、pipe、fork 三项柱状图。
5. **文件 I/O**：iozone parent throughput 横向条形图，突出 read/pread 与 write/rewrite 差异。
6. **问题与改进**：VMA 写回告警、fsync EIO、测试脚本路径三项；附修复后回归结果。

## 11. 展示时必须保留的限制说明

- 顶层命令退出码 0 只表示测试脚本跑完，不表示所有 LTP 断言通过。
- cyclictest 明确提示高精度定时器不可用；结果只能用于同环境相对比较。
- iozone 自动模式因 fsync EIO 中断，写入吞吐不能宣称为完整的持久化性能。
- lmbench shell fork 子项存在错误路径，应复测后再用于横向对比。
- 本轮使用 8 GiB QEMU 内存；与更小内存配置比较时必须重新跑同一套命令。
