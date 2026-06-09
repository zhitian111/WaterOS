# WaterOS TodoList

本文件用于维护阶段性目标、模块开发计划和后续新增任务入口。它不是单次任务记录，而是面向当前阶段目标的持续计划表。

**事实来源**：`os/Cargo.toml`、`os/feature-tree.txt`、各一级组件聚合 `src/lib.rs` 与对应 `Cargo.toml`、各子系统实际实现代码（`os/components/` 下源码）。

## 当前阶段目标

- **RISC-V QEMU 主线**：在已有 ~80 个 syscall（含信号、凭证、网络 socket 族、futex、poll、**ioctl TTY 子集**）和完整 fd-session（dup/fork 继承/CLOEXEC）的基础上，继续解锁测例集注释并将 basic/lua/benchmark 各组从「已接线但注释」变为「通过验收」。**busybox 组（glibc+musl）已于 2026-06-10 验收 55/55。**
- **LoongArch64**：LoongArch64 virt 板级已具备与 RISC-V **相同**的 bring-up 总线（三级页表、virtio 块设备、ext4 挂载、用户 ELF 加载、VFS 桥接），需继续补齐验证覆盖并与 RISC-V 对齐赛题评测环境。
- **赛题脚手架**：多 virtio-blk、virtio-net、RTC、`*_testcode.sh` 串行调度与 START/END 输出、SBI 关机，与赛题发布 QEMU 命令对齐。
- **文档同步**：当前现有文档（`docs/roadmap/` 下各阶段计划与工作包）落后于实现进度，需在完成里程碑后按实际代码状态增量刷新。

## 一级组件推进情况

| 组件 | 当前状态 | 下一步重点 |
|------|----------|------------|
| wateros-platform | API/impl/聚合模式稳定；默认 `impl-qemu-riscv64-opensbi`；LoongArch64 virt 具备完整的 **`mm::kernel_mm::init`（三级页表）、`driver::active_impl::init_after_boot`（virtio 块设备）、`fs::init`（ext4）、`user_bringup_bus::run()`（用户 ELF 加载）** 路径，与 RISC-V 共享同一套 bring-up 总线 | LoongArch 验证覆盖补齐；完整平台能力说明 |
| wateros-driver | 默认路径含 **virtio-mmio 块设备** + DTB 扫描；block/character/network API 已定义；**virtio-net 已可识别 device_id，smoltcp 协议栈已集成**（`driver::network::stack::init` + poller task） | 充实字符设备实现；网络栈错误路径与多实例策略 |
| wateros-mm | **`impl-sv39`**、帧分配 **`impl-stack`**、内核页表已稳定；**用户态 `brk`/`mmap`/`munmap`/`mprotect`** 已全部接线到 syscall 层并真实操作用户地址空间（含 `user_aspace_ptr`）；`from_elf_path` 可完整装载 ELF | LoongArch 侧避免直接照搬 Sv39 假设；`UserMemoryOps` 安全拷贝持续测试 |
| wateros-runtime | console/logging/panic/heap allocator 子 crate 已接入；`pub` 与模块级 rustdoc 已补齐 | 随子 impl 或默认 feature 变更同步文档 |
| wateros-fs | 默认 **`impl-ext4`**（RO + RW beta）；**devfs/rootfs 的 `impl-kernel`**；与驱动协作完成根块探测与挂载 | 多根设备策略、挂载协议扩展 |
| wateros-vfs | **`impl-fs-bridge`**（VFS 桥接）稳定；**`impl-fd-session`** 已完整支持 per-task fd 表（dup/dup3/fork 继承/CLOEXEC 全部实现+测试） | 路径/会话语义与 fs 侧 RW/RO 视图一致性；VFS 自检回归 |
| wateros-ipc | 聚合层默认含 **waitqueue**、**pipe**（内核 ring-buffer + fd endpoint）；**futex** 通过 dispatch 表接入（WAIT/WAKE）；signal 相关结构已构建但用户态 handler 路径待联调 | signal handler trap 返回路径完整验证；shm/event 继续推进 |
| wateros-task | **`impl-core` + 轮转调度**；用户任务 spawn 完整（`spawn_user_task_from_loaded_elf`）；阻塞/睡眠队列、WaitQueue、zombie 回收、最小父子关系与 wait 服务 | trap 驱动抢占；TaskHandle generation；跨架构文档 |
| wateros-abi | **`api-v0`** 与 **`impl-linux-generic64`** 默认启用；errno、号表、参数与 `UserRet` 已供 syscall 使用 | 调用号与内核实际支持集合对齐；**`SYSLOG` (116)** 待 klog 落地时加入号表 |
| wateros-klog | **已落地**（[`docs/architecture/wateros-klog.md`](../architecture/wateros-klog.md)） | `CONSOLE_*` 接 runtime-console；权限；`/dev/kmsg` 线格式 |
| wateros-syscall | **独立一级 crate**，RISC-V 主线默认链接；dispatch 表已覆盖以下单元：read/write/writev/readlinkat/openat/close/lseek/fstat/dup/dup3/pipe2/brk/mmap/munmap/mprotect/gettimeofday/clock_gettime/getpid/getppid/gettid/getuid/geteuid/getgid/getegid/getgroups/setuid/setgid/setreuid/setregid/setresuid/setresgid/futex/fcntl/clone/execve/waitpid/kill/nanosleep/times/getcwd/chdir/mkdirat/getdents64/unlinkat/mount/umount2/uname/prctl/getrlimit/setrlimit/prlimit64/set_tid_address/set_robust_list/getrandom/rt_sigaction/rt_sigprocmask/socket/bind/listen/accept4/connect/getsockname/getpeername/sendto/recvfrom/sendmsg/recvmsg/setsockopt/getsockopt/shutdown/poll + statx（未知号路由）+ exit/exit_group/yield + **ioctl（TTY 子集）**。 | basic 测例全解锁；lua/benchmark 脚本验收 |
| wateros-cred | **代码已实现**——`cred-api` + `impl-root`；dispatch 表含 getuid/geteuid/getgid/getegid/getgroups/setuid/setgid/setreuid/setregid/setresuid/setresgid；fork/exec 生命周期已接入 | VFS stat 占位；与 ext4 inode owner 对接 |
| wateros-base | 基础类型与 **base-config**（含 MM 相关常量） | 避免向上层泄漏板级魔法数 |
| wateros-utils | 通用轻量工具 | 保持无跨层耦合 |

## 当前优先任务

- **basic 测例全解锁**：`os/src/user_bringup_basic.rs` 中当前仅启用 8 个测例（clone/fork/wait/waitpid/getpid/getppid/exit/execve），其余 20+ 个为注释状态；需要逐项取消注释并修复边界失败。
- **lua 脚本解锁**：`user_bringup_busybox.rs` 中 P2 lua 路径仍注释；P2 busybox 已于 2026-06-10 通过（glibc/musl 各 55/55，日志 `/tmp/wateros_P2_busybox.log`）。
- **devfs `/dev/null`**：busybox 后台 job 重定向时报 `can't open '/dev/null'`（不影响当前 55/55 判读，但应补节点）。
- **赛题脚手架**：多盘、RTC、关机、`*_testcode.sh` 串行调度与 START/END 输出，与 `testsuits-for-oskernel/README.md` 评测命令对齐。

## 赛题 test_case 全通过专项

分阶段路线见 **`docs/roadmap/test-case-full-pass-plan.md`**（当前文档落后于实际进度，后续需以实际代码状态重写阶段划分）。

## RISC-V64 BusyBox bring-up（并行工作包）

见 **`docs/roadmap/riscv64-busybox/README.md`**。**注意**：该目录下的工作包文档描述的「缺口」多数已在代码中实现（如 fd-session dup/fork 继承、信号 dispatch、进程凭证、网络 socket 族等），当前瓶颈在「解锁注释 + 修复暴露的 bug」而非「从零实现」。整体劳动量预计缩减至 8-12 周。

## 后续阶段占位（待拆分）

以下条目用于承接跨组件或尚未立项的大块工作，在具体任务文件中拆分为可评审步骤：

- LoongArch64 验证覆盖补齐与赛题环境对齐。
- LTP 全量遍历与分桶收敛。
- glibc/musl x riscv/loongarch 四套 sdcard 交叉验证与 CI 策略。

## 新增任务入口

新增任务时请至少补充以下信息：

- 目标组件（可写到上表「下一步」或单独 issue/任务 md）
- 任务类型：设计、实现、文档、重构、验证
- 是否依赖某个 `api-v0`
- 是否需要新增 `impl-*` 或根/组件 feature
- 预计同步更新：`docs/roadmap/todolist.md`、`docs/architecture/snapshot.md`、`docs/exports/`、`docs/guides/` 中的哪些路径