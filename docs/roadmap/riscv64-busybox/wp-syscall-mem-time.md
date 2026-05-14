# 工作包：wateros-syscall + mm — 内存与时间类系统调用

**所属**：`wateros-syscall`、`wateros-mm`、`wateros-platform`（时钟源）。  
**并行度**：与 **文件 IO syscall** 可并行开发；**`mmap` 与 exec 装载** 与 `wp-syscall-process-exec.md` 存在合并点，需接口人协调。

## 要做什么

1. **内存**：实现或接通 `mmap`、`munmap`、`mprotect`（与 `wp-mm-user-riscv64.md` 一致）；处理 `MAP_ANONYMOUS`、`MAP_FIXED` 的 **最小子集**（拒绝不支持的 flag 时返回 `EINVAL`/`ENOTSUP` 并与 musl/glibc 探测行为兼容）。
2. **`brk`**：删除或收窄全局原子桩，改为 **每任务 brk**（由 MM 或 task 持有），与 `wp-mm-user-riscv64.md` 决策一致。
3. **时间与休眠**：`gettimeofday`、`clock_gettime`（CLOCK_REALTIME/MONOTONIC 至少其一）、`nanosleep` 或 `clock_nanosleep` 之一，满足 `basic`/脚本中 `sleep` 的常见路径。
4. **信息类**：`uname`（固定字段可接受占位）、`getpid`/`getppid`（可先单进程树简化）。

## 验收要求

- [ ] 用户程序：`mmap` 匿名页 → 写入 → `munmap` → 再次 `mmap` 得到新页或明确错误，无内核 panic。
- [ ] `nanosleep` 短间隔（如 1ms）在 QEMU 下可观测 **调度让出**（日志 tick 或计数增加）。
- [ ] `uname` 返回非空 release/machine 字段，BusyBox 探测不因全零崩溃。

## 验证方式

1. bring-up 总线阶段 **`[bringup][syscall-mem-time]`**：运行内联用户测例或独立 ELF，打印 `mmap` 写入魔数与 `uname` 单行摘要。
2. 与 **文件 IO** 阶段独立：测例 ELF 不依赖 `open` 亦可运行（纯匿名 mmap），便于并行调试。
3. 时间 syscall：与 `platform::timer` 日志对照，确认单调性（允许 QEMU 抖动，但无时间倒流级错误）。

## 依赖

- **上游**：`wp-mm-user-riscv64.md`。
- **下游**：`wp-syscall-process-exec.md`（动态装载）、`wp-ash-job-control.md`（sleep/alarm 类）。

## 可并行对象

`wp-syscall-file-io.md`（不同 syscall 号分支）；`wp-platform-driver-scaffold.md` 中 RTC 与 wall-clock 对齐（若选 REALTIME）。
