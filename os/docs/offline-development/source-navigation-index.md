# 全组件源码导航与搜索索引

本文不是 API 列表，而是现场排障的“第一条 `rg` 命令”。路径均相对 `os/`。先进入状态所有者，再沿调用者向上追到 syscall/bring-up；不要从日志字符串盲目全仓修改。

## 通用搜索方法

```sh
# 找定义、实现和调用
rg -n "pub (struct|enum|trait|fn) NAME|impl .*NAME|NAME\(" components src

# 找全局状态与锁
rg -n "static .*Mutex|static .*RwLock|Once|CpuLocal|Atomic" components/wateros-<name>

# 找创建、继承和释放
rg -n "spawn|create|clone|fork|exec|exit|drop|release|reap|abort" components/wateros-<name>

# 找错误转换
rg -n "map_.*err|to_errno|ErrNo::|VfsError::|DriverError::" components/wateros-<name>

# 找 feature 分支
rg -n '#\[cfg|compile_error!' components/wateros-<name> os/Cargo.toml
```

看到 trait 方法时先找当前 feature 选择的 active impl，不要把未启用的备用实现当实际运行路径。`make show-config` 用于确认架构、profile、mode 和 feature 来源。

仓库中可能残留空目录（例如历史 `impl-dummy`、`wateros-abi` 或 `wateros-pseudo-shell` 骨架）。Git
不跟踪空目录，某台开发机上“看得见目录”不证明它是当前模块。判定有效组件以这三项为准：

1. 目录中有受版本控制的 `Cargo.toml`/源码；
2. 上级 workspace `members` 或依赖 path 包含它；
3. 当前 feature tree 实际选择它。

可用 `git ls-files <path>`、`find <path> -name Cargo.toml` 和 `cargo tree -e features` 交叉确认。
不要为只有本地空骨架的目录补实现，也不要把里面残留的独立 `Cargo.lock/target` 当源码证据。

## base：容量、CPU 标识与基础同步

| 目标 | 入口 |
| --- | --- |
| 全局容量和策略常量 | [`base-config/src`](../../components/wateros-base/base-config/src) |
| CPU ID/mask 与 per-CPU 容器 | [`src`](../../components/wateros-base/src) |
| 同步辅助 | [`src/sync`](../../components/wateros-base/src/sync) |

先搜：

```sh
rg -n "MAX_CPUS|KERNEL_HEAP_SIZE|KLOG_|SCHED_|PAGE_" components/wateros-base/base-config/src
rg -n "CpuId|CpuMask|CpuLocal|Once" components/wateros-base
```

改容量前找所有依赖它的固定数组和位宽。配置值是跨组件契约，不应在消费者重复写 magic number。

## platform：ISA、trap、timer、IPI 和 reset

| 层 | 入口 |
| --- | --- |
| 稳定平台 API | [`platform-api/api-v0/src`](../../components/wateros-platform/platform-api/api-v0/src) |
| ISA 原语选择 | [`platform-arch/src/lib.rs`](../../components/wateros-platform/platform-arch/src/lib.rs) |
| RISC-V OpenSBI profile | [`impl-qemu-riscv64-opensbi`](../../components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi) |
| LoongArch virt profile | [`impl-qemu-loongarch64-virt`](../../components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt) |
| 组合层 trap 路由 | [`src/trap_handler.rs`](../../src/trap_handler.rs) |

```sh
rg -n "trap_entry|TrapContext|TrapCause|set_timer|interrupt|send_ipi|shootdown|reset" components/wateros-platform src/trap_handler.rs
```

ISA 负责 frame/CSR/汇编边界，组合层负责 syscall、fault、signal、scheduler tick。修改 trap frame 后必须同步汇编 offset、Rust layout assertion、signal frame 和 GDB 解码。

## runtime：最小运行环境

| 子模块 | 入口 |
| --- | --- |
| console | [`runtime-console/src/lib.rs`](../../components/wateros-runtime/runtime-console/src/lib.rs) |
| logging | [`runtime-logging/src`](../../components/wateros-runtime/runtime-logging/src) |
| heap | [`runtime-heap-allocator/src`](../../components/wateros-runtime/runtime-heap-allocator/src) |
| panic | [`runtime-panic/src/lib.rs`](../../components/wateros-runtime/runtime-panic/src/lib.rs) |
| serial | [`runtime-serial/src/lib.rs`](../../components/wateros-runtime/runtime-serial/src/lib.rs) |

```sh
rg -n "global_allocator|HEAP_SPACE|with_allocator_interrupt_guard|set_logger|panic_handler|write_fmt" components/wateros-runtime
```

这条路径在 task/VFS 可用前就会运行；任何新增依赖都要先证明早期启动可用且不会分配/阻塞。

## task：调度实体、进程与回收

| 目标 | 入口 |
| --- | --- |
| 聚合生命周期 | [`wateros-task/src`](../../components/wateros-task/src) |
| TCB/process registry | [`task-impl/impl-core`](../../components/wateros-task/task-impl/impl-core) |
| scheduler API/实现 | [`task-scheduler`](../../components/wateros-task/task-scheduler) |
| syscall 编排 | [`sys/task`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task) |

```sh
rg -n "TaskControlBlock|ProcessRecord|mark_task_exited|reap_exited|run_first_task|schedule|wait_current" components/wateros-task
rg -n "start_fork_child|abort_fork_child|terminate_other_threads_for_exec" components/wateros-task components/wateros-syscall
```

区分 TaskId/TID/PID，并区分 exit 与 reap。调度锁内不能执行用户 copy、VFS、复杂释放或可睡眠操作。

## mm：地址空间、VMA、页表和帧

| 目标 | 入口 |
| --- | --- |
| 跨实现契约 | [`mm-api/api-v0/src`](../../components/wateros-mm/mm-api/api-v0/src) |
| 物理帧 | [`mm-frame-alloctor`](../../components/wateros-mm/mm-frame-alloctor) |
| 共用 MM 逻辑 | [`mm-impl/common`](../../components/wateros-mm/mm-impl/common) |
| Sv39 | [`mm-impl/impl-sv39`](../../components/wateros-mm/mm-impl/impl-sv39) |
| LoongArch | [`mm-impl/impl-loongarch64`](../../components/wateros-mm/mm-impl/impl-loongarch64) |

```sh
rg -n "Vma|mmap|munmap|mprotect|page_fault|lazy|Cow|writeback|drop_user_aspace|shootdown" components/wateros-mm
rg -n "copy_(to|from)_user|UserMemoryOps|translate" components/wateros-mm components/wateros-syscall
```

同一语义的两架构实现必须同时审查。先判断 bug 属于 VMA 元数据、PTE、frame refcount、TLB 还是 VFS file identity。

## VFS：路径、fd、OFD 与页缓存

| 目标 | 入口 |
| --- | --- |
| handle/path/fd 契约 | [`vfs-api/api-v0/src`](../../components/wateros-vfs/vfs-api/api-v0/src) |
| per-task fd session | [`impl-fd-session`](../../components/wateros-vfs/vfs-impl/impl-fd-session) |
| FS/mount/proc bridge | [`impl-fs-bridge`](../../components/wateros-vfs/vfs-impl/impl-fs-bridge) |
| page cache | [`impl-page-cache`](../../components/wateros-vfs/vfs-impl/impl-page-cache) |

```sh
rg -n "VfsIoHandle|VfsPreparedRead|duplicate|alloc_fd|close_slot|share_fd_table|copy_fd_table" components/wateros-vfs
rg -n "PagedFileHandle|writeback|mount_generation|resolve|symlink" components/wateros-vfs
```

fd slot flag 与 OFD status flag 不同。消费型 read 必须检查 prepare/acquire/finish，文件映射必须检查 handle/identity 生命周期。

## fs：磁盘格式与伪文件系统

| 目标 | 入口 |
| --- | --- |
| FS trait | [`fs-api/api-v0/src`](../../components/wateros-fs/fs-api/api-v0/src) |
| 当前默认 ext4 | [`impl-another-ext4`](../../components/wateros-fs/fs-impl/impl-another-ext4) |
| 备用 ext4 | [`impl-ext4`](../../components/wateros-fs/fs-impl/impl-ext4)、[`impl-ext4-rs`](../../components/wateros-fs/fs-impl/impl-ext4-rs) |
| ramfs | [`impl-ramfs`](../../components/wateros-fs/fs-impl/impl-ramfs) |
| rootfs/devfs/procfs | [`fs-rootfs`](../../components/wateros-fs/fs-rootfs)、[`fs-devfs`](../../components/wateros-fs/fs-devfs)、[`fs-procfs`](../../components/wateros-fs/fs-procfs) |

```sh
rg -n "registered_fs_impls|probe\(|mount_(ro|rw)|ROOT_(RW_)?FS|sync|flush|write_at|truncate|rename" components/wateros-fs
```

确认实际 feature 后再修改 backend。procfs 数据来自 callback 快照，不应直接反向依赖所有业务组件。

## driver：设备枚举与注册

| 目标 | 入口 |
| --- | --- |
| 机器契约 | [`driver-api/api-v0/src/lib.rs`](../../components/wateros-driver/driver-api/api-v0/src/lib.rs) |
| 公共 DTB 解析 | [`impl-common`](../../components/wateros-driver/driver-impl/impl-common) |
| RISC-V / LoongArch probe | [`impl-qemu-riscv64-virt`](../../components/wateros-driver/driver-impl/impl-qemu-riscv64-virt)、[`impl-qemu-loongarch64-virt`](../../components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt) |
| 设备类 | [`driver-block`](../../components/wateros-driver/driver-block)、[`driver-network`](../../components/wateros-driver/driver-network)、[`driver-character`](../../components/wateros-driver/driver-character)、[`driver-display`](../../components/wateros-driver/driver-display)、[`driver-input`](../../components/wateros-driver/driver-input) |

```sh
rg -n "scan_device_info|probe_.*devices|register_.*device|supported_devices|from_mmio|from_pci" components/wateros-driver
```

依次证明 QEMU device、DTB/PCI、claim、实例化、registry、devfs 和上层 consumer，不能跨级推断。

## IPC：等待与消息传递机制

| 子模块 | 入口 |
| --- | --- |
| waitqueue | [`ipc-waitqueue`](../../components/wateros-ipc/ipc-waitqueue) |
| futex | [`ipc-futex`](../../components/wateros-ipc/ipc-futex) |
| pipe | [`ipc-pipe`](../../components/wateros-ipc/ipc-pipe) |
| signal | [`ipc-signal`](../../components/wateros-ipc/ipc-signal) |
| SHM | [`ipc-shm`](../../components/wateros-ipc/ipc-shm) |
| event primitives | [`ipc-event`](../../components/wateros-ipc/ipc-event) |

```sh
rg -n "WaitQueue|wait_current_while|wake_|reservation|cancel|close|Drop" components/wateros-ipc
rg -n "sysv|eventfd|signalfd|robust|SEM_UNDO" components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc
```

所有阻塞路径都要检查 lost wakeup、虚假唤醒、signal/timeout/exit 取消和对象最后 owner 释放。

## cred：身份与能力

| 目标 | 入口 |
| --- | --- |
| API | [`cred-api/api-v0`](../../components/wateros-cred/cred-api/api-v0) |
| registry/钩子 | [`cred-impl/impl-root`](../../components/wateros-cred/cred-impl/impl-root) |
| ABI | [`sys/cred`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred) |

```sh
rg -n "Credential|Uid|Gid|cap|fork_cred|share_cred|on_exec|drop_task_cred|permission" components/wateros-cred components/wateros-syscall
```

凭证在 zombie reap 前仍可能被退出收尾查询。setuid/setgid、saved ID、fs ID 和 capability 必须一起审查。

## syscall：Linux ABI 组合层

| 目标 | 入口 |
| --- | --- |
| generic64 number/type | [`syscall-api/api-v0`](../../components/wateros-syscall/syscall-api/api-v0) |
| dense dispatch | [`syscall_nr_dispatch.rs`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs) |
| 用户复制 | [`user_copy.rs`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/user_copy.rs) |
| 九个 domain | [`src/sys`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys) |

```sh
rg -n "const .*: usize|sys_[a-z0-9_]+|SyscallArgs|UserRet|copy_(to|from)_user|ErrNo" components/wateros-syscall
```

handler 只负责编排、ABI 和错误转换。长期状态应放入 task/MM/VFS/IPC/network/cred 等所有者。

## network：协议栈与 socket fd

| 目标 | 入口 |
| --- | --- |
| 公共类型 | [`network-api/api-v0`](../../components/wateros-network/network-api/api-v0) |
| smoltcp stack | [`impl-smoltcp`](../../components/wateros-network/network-impl/impl-smoltcp) |
| socket 对象/fd/lease | [`src/socket`](../../components/wateros-network/src/socket) |
| socket syscall | [`sys/net`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net) |

```sh
rg -n "NetworkStack|StackSocketHandle|SocketRef|SocketReceiveLease|poll_socket_events|socket_(send|recv|accept|close)" components/wateros-network
```

协议栈需要 poller 推进。socket fd 的最后关闭由共享 Arc 决定，用户 copy 前的接收只能预留不能消费。

## tty：终端与 PTY

| 目标 | 入口 |
| --- | --- |
| termios/事件契约 | [`tty-api/api-v0`](../../components/wateros-tty/tty-api/api-v0) |
| console line discipline/PTY | [`tty-impl/impl-console`](../../components/wateros-tty/tty-impl/impl-console) |
| VFS handle | [`impl-fd-session`](../../components/wateros-vfs/vfs-impl/impl-fd-session) |

```sh
rg -n "feed_input|prepare_read|finish_read|foreground_pgid|controlling_sid|Pty|hangup|termios" components/wateros-tty components/wateros-vfs
```

TTY 锁内只变更状态，echo、信号投递、等待和设备 I/O 都在锁外。

## klog 与 debug：留存和停机诊断

| 目标 | 入口 |
| --- | --- |
| klog ring | [`klog-impl/impl-kernel`](../../components/wateros-klog/klog-impl/impl-kernel) |
| klog ABI | [`klog-api/api-v0`](../../components/wateros-klog/klog-api/api-v0) |
| debug ABI/TrackedMutex | [`wateros-debug/src`](../../components/wateros-debug/src) |
| 主机调试脚本 | [`scripts/debug`](../../scripts/debug) |

```sh
rg -n "KlogRingbuf|read_cursor|records_dropped|record_event|publish_cpu_state|TrackedMutex|WATEROS_DEBUG" components/wateros-klog components/wateros-debug scripts/debug
```

klog view 只在锁内有效；debug 事件编号是主机 ABI，只能追加。两条热路径都不能分配或递归日志。

## GUI：内核软件合成器

| 目标 | 入口 |
| --- | --- |
| 数据模型 | [`gui-api/api-v0`](../../components/wateros-gui/gui-api/api-v0) |
| runtime/scene/input/surface | [`gui-impl/impl-software`](../../components/wateros-gui/gui-impl/impl-software) |
| 启动任务 | [`src/main.rs`](../../src/main.rs) |

```sh
rg -n "GuiRuntime|ShadowSurface|DirtyRegions|Desktop|InputBridge|render_if_dirty|flush_region" components/wateros-gui src/main.rs
```

先确认 `gui` 与 `user-graphics` 所有权选择。锁顺序只允许 GUI runtime 到短暂 display/input device lock。

## utils：纯工具

| 目标 | 入口 |
| --- | --- |
| 聚合 | [`wateros-utils/src/lib.rs`](../../components/wateros-utils/src/lib.rs) |
| table format | [`table-format/src`](../../components/wateros-utils/table-format/src) |

```sh
rg -n "pub (struct|enum|fn)|unsafe|platform|task|mm|vfs" components/wateros-utils
```

若出现 platform/task/MM/VFS 依赖，通常说明工具放错层。纯工具应能 host test、无全局初始化并由调用方提供输出目标。

## 从错误日志反向定位

```sh
# 精确搜日志模板，不要带动态地址/数字
rg -n -F '[heap] OOM' .
rg -n -F 'AP online timeout' .
rg -n -F 'writeback' components src

# 找 panic 所在函数的所有调用者
rg -n "function_name\(" components src

# 查一个 errno 从哪里产生和被转换
rg -n "ErrNo::EFAULT|VfsError::Fault|copy_to_user" components/wateros-syscall components/wateros-vfs
```

定位后记录“最后成功状态”和“第一个失败状态”。后续日志可能只是级联结果，例如 QEMU hostfwd 失败发生在内核运行前，不能据此修改内核网络栈。
