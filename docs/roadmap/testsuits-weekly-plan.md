# testsuits-for-oskernel 全通过 — 按周实施计划表

**基于 2026-06 实际代码分析**，核心工作从「从零实现内核功能」转变为 **「解锁注释 + 修复暴露的 bug」** 。当前 RISC-V 主线已有 ~91 个 syscall 接线、完整 fd-session、用户态 MM、信号骨架、网络 socket 族等。

> **预估总工期：~10-12 周**（1-2 人全职）

---

## 总览

```
周次:  1  2  3  4  5  6  7  8  9 10 11 12
P1    ██ ██
P2        ██ ██
P3              ██ ██
P4                    ██
P5                       ██ ██
P6                             ██ ██ ██
```

| 阶段 | 聚焦 | 周次 | 工作量 |
|------|------|------|--------|
| **P1** | 回归基线 + ioctl + basic首批 | W1-W2 | 2 人周 |
| **P2** | basic全表 + busybox + lua | W3-W4 | 2-3 人周 |
| **P3** | benchmark + 网络 | W5-W6 | 2-3 人周 |
| **P4** | 赛题脚手架 + cyclictest | W7 | 1 人周 |
| **P5** | LTP + libctest | W8-W9 | 2 人周 |
| **P6** | LoongArch 验证覆盖 + 交叉验证 | W10-W12 | 2-3 人周（建议独立人力） |

---

## 第 1 周：回归基线 & ioctl 补齐

### 目标
- 回归基线确认（`make all` + QEMU riscv64 可跑）
- `dispatch_ioctl` 实现
- 恢复被注释的 bring-up 阶段

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 1.1 | 回归基线确认 | 全栈 | 4h |
| 1.2 | 补 `dispatch_ioctl` override | `impl-kernel/src/lib.rs` | 2h |
| 1.3 | 实现 TTY ioctl 子集（TCGETS、TIOCGPGRP、TIOCGWINSZ 等） | `impl-kernel/src/sys/ioctl.rs`（新建） | 1d |
| 1.4 | 恢复 `stage-02-mm` 注释 | `user_bringup_bus.rs` | 0.5h |
| 1.5 | 恢复 `stage-posix-fs-meta` 注释 | `user_bringup_bus.rs` | 0.5h |
| 1.6 | basic 首批 12 个测程解锁 | `user_bringup_basic.rs` | 1h |
| 1.7 | 修复首批测程暴露的失败 | 各模块 | 2-3d |

### 首批解锁的 basic 测程
```
chdir  close  fstat  getcwd  gettimeofday
open   openat read   write   yield
sleep  times  uname  test_echo
```

### 验收标准
- [ ] `make all` 成功生成 `kernel-rv`
- [ ] QEMU riscv64 内核启动并通过自检
- [ ] `[basic-bringup]` 日志对 14 个 ELF 有加载→执行→退出记录（含 glibc + musl）

---

## 第 2 周：basic 首批修复 & 回归稳定

### 目标
- 第 1 周暴露的问题全部修复
- basic 首批测程稳定通过

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 2.1 | 解决用户态 `openat`/`read`/`write` 路径问题 | vfs/syscall | 1-2d |
| 2.2 | 解决 `chdir`/`getcwd` per-task cwd 问题 | syscall/task | 1d |
| 2.3 | 解决 `fstat`/`gettimeofday` 返回值问题 | syscall | 1d |
| 2.4 | `stage-02-mm` 验证（brk/mmap/munmap 烟测） | mm/bringup | 1d |
| 2.5 | `stage-posix-fs-meta` 验证 | fs/bringup | 1d |
| 2.6 | 记录未通过测程与原因 | 文档 | 0.5d |

### 验收标准
- [ ] basic 首批 14 个测程全部通过（glibc + musl）
- [ ] `[mm-bringup]` 日志显示 brk/mmap/munmap ELF 成功装载运行
- [ ] 已知失败的测程有明确的根因记录

---

## 第 3 周：basic 全表 & dup/pipe 解锁

### 目标
- basic 剩余测程解锁并通过
- dup/dup2/pipe/getdents/mkdir/unlink 验证

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 3.1 | basic 第二批解锁（dup dup2 getdents mkdir_ pipe unlink） | `user_bringup_basic.rs` | 1h |
| 3.2 | 修复 dup/dup3 边界问题 | vfs-fd/syscall | 1d |
| 3.3 | 修复 pipe fork 组合问题 | ipc/syscall | 1d |
| 3.4 | 修复 getdents64 目录遍历问题 | syscall | 1d |
| 3.5 | basic 第三批解锁（mount umount mnt） | `user_bringup_basic.rs` | 1h |
| 3.6 | 修复 mount/umount 路径问题（单盘下 ENOENT 处理） | syscall/vfs | 1d |

### 验收标准
- [ ] basic 24 测程全部通过（glibc + musl）
- [ ] `dup`/`dup2`/`pipe`/`getdents`/`mkdir_`/`unlink` 逐个验证

---

## 第 4 周：busybox + lua

### 目标
- busybox 多脚本跑通
- lua 解释器可执行

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 4.1 | 解锁 `/musl/basic_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 4.2 | 解锁 `/glibc/busybox_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 4.3 | busybox echo/sh -c 探针验收 | bringup | 1d |
| 4.4 | 修复 busybox ash 执行问题（ioctl TTY 相关） | syscall | 1-2d |
| 4.5 | 解锁 `/glibc/lua_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 4.6 | 修复 lua 运行问题（动态链接/文件 IO） | mm/syscall/vfs | 1-2d |

### 验收标准
- [ ] `busybox echo __ok__` 探针通过
- [ ] `busybox sh -c 'echo hello'` 探针通过
- [ ] busybox_testcode.sh 脚本开始运行
- [ ] lua 解释器可执行简单 `.lua` 文件

---

## 第 5 周：benchmark 组

### 目标
- lmbench/unixbench/libcbench/iozone 跑通

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 5.1 | 解锁 `/glibc/lmbench_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 5.2 | 修复 lmbench 失败项（signal/select/pipe 带宽） | syscall/ipc | 1-2d |
| 5.3 | 解锁 `/glibc/unixbench_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 5.4 | 修复 unixbench 失败项（多进程/调度） | task/syscall | 1d |
| 5.5 | 解锁 `/glibc/libcbench_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 5.6 | 解锁 `/glibc/iozone_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 5.7 | 修复 iozone 失败项（preadv/pwritev/fsync） | syscall/vfs | 1-2d |

### 验收标准
- [ ] 各组 benchmark 脚本开始运行
- [ ] 至少 50% 的测项通过
- [ ] 失败项有明确的根因记录

---

## 第 6 周：网络组

### 目标
- iperf/netperf loopback 跑通

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 6.1 | 解锁 `/glibc/iperf_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 6.2 | 解锁 `/glibc/netperf_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 6.3 | 验证 smoltcp loopback TCP/UDP | driver::network | 1d |
| 6.4 | 修复 iperf3 server 后台进程模型 | task/syscall | 1-2d |
| 6.5 | 修复 netserver 后台进程模型 | task/syscall | 1d |
| 6.6 | QEMU 网络参数验证 | 启动脚本 | 1d |

### 验收标准
- [ ] iperf3 loopback TCP 测试通过
- [ ] netperf TCP_STREAM 测试通过

---

## 第 7 周：赛题脚手架 + cyclictest

### 目标
- 赛题 QEMU 环境对齐
- 关机路径
- cyclictest

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 7.1 | 更新 `os/scripts/test_in_qemu_riscv.sh` 与赛题命令对齐 | 启动脚本 | 1d |
| 7.2 | `*_testcode.sh` 串行调度 + START/END 输出 | bringup 总线 | 1d |
| 7.3 | SBI 关机/退出 QEMU 路径确认 | platform | 1d |
| 7.4 | `make all` → `kernel-rv`/`kernel-la` 产物确认 | Makefile | 0.5d |
| 7.5 | 解锁 `/glibc/cyclictest_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 7.6 | 修复 cyclictest 失败项（高精度定时器/SIGINT） | syscall/task | 1-2d |

### 验收标准
- [ ] QEMU 命令与赛题 `testsuits-for-oskernel/README.md` 一致
- [ ] 内核输出 `#### OS COMP TEST GROUP START basic ####` / `END` 格式
- [ ] 测试结束后 QEMU 自动退出

---

## 第 8 周：LTP 收敛

### 目标
- LTP 子集通过
- libctest 通过

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 8.1 | 解锁 `/glibc/libctest_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 8.2 | 修复 libctest 动态/静态链接问题 | mm/loader | 2d |
| 8.3 | TLS 支持验证 | mm/task | 1d |
| 8.4 | 解锁 `/glibc/ltp_testcode.sh` | `user_bringup_busybox.rs` | 1h |
| 8.5 | LTP 分桶收敛，先跑文件/IPC 类 | 各模块 | 2-3d |

### 验收标准
- [ ] libctest 静态链接测程通过
- [ ] LTP 文件/IPC 子集通过 ≥ 50%

---

## 第 9 周：LTP 扩展 & musl 全量

### 目标
- LTP 更多子集通过
- musl 路径全量解锁

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 9.1 | LTP 扩展到进程/内存/调度子集 | 各模块 | 2d |
| 9.2 | 修复 LTP 暴露的系统性问题 | 各模块 | 1-2d |
| 9.3 | 解锁全部 `/musl/` 路径脚本 | `user_bringup_busybox.rs` | 1h |
| 9.4 | 修复 musl 差异问题 | syscall/abi | 1-2d |

### 验收标准
- [ ] LTP 整体通过率 ≥ 30%
- [ ] musl 路径 basic/busybox 通过

---

## 第 10-12 周：LoongArch 验证覆盖 + 交叉验证

**注意**：LoongArch64 已具备与 RISC-V **相同**的 bring-up 总线（三级页表、块设备、ext4 挂载、用户 ELF 加载、VFS 桥接），并非从零实现。其工作重心在验证覆盖补齐。

### 目标
- LoongArch 验证覆盖补齐（syscall dispatch 在 loongarch 上的完整运行）
- 四套 sdcard 交叉验证
- CI 策略

### 任务清单

| 编号 | 任务 | 文件/模块 | 预估工时 |
|------|------|-----------|----------|
| 10.1 | LoongArch 验证覆盖：运行 RISC-V 相同的 basic/busybox 测程确认差异 | 全栈 | 1 周 |
| 10.2 | LoongArch 赛题环境对齐（QEMU 多盘/RTC/关机） | platform/scripts | 1 周 |
| 10.3 | riscv/loongarch × glibc/musl 四套抽样验证 | 验证 | 1 周 |
| 11.1 | 构建 CI 自动化（Makefile + QEMU 自动验证） | CI | 1 周 |
| 11.2 | 剩余 LTP 子集收敛 | 各模块 | 1 周 |
| 12.1 | 全量回归 & 文档同步 | 文档 | 0.5 周 |

### 验收标准
- [ ] LoongArch QEMU 上 basic/busybox 测程通过路径与 RISC-V 一致
- [ ] 四套 sdcard 镜像的 basic/busybox 通过
- [ ] CI 可自动构建 + QEMU 回归

---

## 关键里程碑

| 里程碑 | 周次 | 验收条件 |
|--------|------|----------|
| **M1** | W2 | ioctl 补齐 + basic 首批 14 测程通过 + stage-02-mm/posix-fs-meta 恢复 |
| **M2** | W4 | basic 24 测程全通过 + busybox sh 探针通过 + lua 解释器可运行 |
| **M3** | W6 | benchmark 4 组 + 网络 2 组脚本运行超 50% 测项 |
| **M4** | W7 | 赛题 QEMU 命令对齐 + START/END 输出 + 关机 |
| **M5** | W9 | LTP + libctest 通过 + musl 全量 |
| **M6** | W12 | LoongArch 验证覆盖 + 四套交叉验证 + CI |

---

## 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| basic 测程暴露大量边界 bug | 中 | 高 | W1 先跑回归基线评估困难程度 |
| ioctl 实现复杂（TTY 完整语义） | 中 | 中 | 先实现最小子集（TCGETS/TIOCGPGRP），后续按 strace 增量 |
| smoltcp 网络栈不够成熟 | 中 | 高 | W6 前先做 loopback smoke test 评估 |
| LoongArch 工作任务重 | 高 | 高 | 建议独立人力，P6 与 P1-P5 并行 |
| LTP 用例量巨大 | 高 | 低 | 先收敛文件/IPC 子集，不追求全通过 |

---

## 每日执行建议

1. **每天开始时**：跑一次 `make all` + QEMU 回归基线，确认未退化
2. **每次修改前**：确认影响范围（syscall、vfs、mm、task 等）
3. **每次解锁注释前**：先在 QEMU 中单独运行对应测程
4. **每次修复后**：更新 `docs/roadmap/todolist.md` 和 `test-case-full-pass-plan.md` 勾选清单
5. **每周结束时**：更新本文档的进度状态