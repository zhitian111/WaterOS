# 内核堆（资源 #6）生命周期审计

> 审计时间：2026-06-25  
> 分组 ID：`kernel-heap`  
> 搜索范围：`os/components/wateros-runtime/runtime-heap-allocator/**` 及全组件经 `#[global_allocator]` / `alloc` 的元数据分配路径  
> Baseline：单核多线程；对照 Linux `ENOMEM` 语义  
> 交叉参考：`docs/audits/resource-inventory.md` #6、`docs/audits/lock-inventory.md` #9、`docs/exports/features/wateros-runtime.md`

---

## 1. 资源概要

| 字段 | 内容 |
|------|------|
| 资源名称 | 内核堆（Kernel Heap） |
| 所属组件 | `wateros-runtime` / `runtime-heap-allocator` |
| 主要类型 | `InterruptSafeLockedHeap`（包装 `linked_list_allocator::LockedHeap`） |
| 物理载体 | 静态 BSS 数组 `HEAP_SPACE: [u8; KERNEL_HEAP_SIZE]`，链接符号 `kernel_heap` |
| 硬上限 | **128 MiB**（`KERNEL_HEAP_SIZE_BIT_WIDTH = 27`，`wateros-base-config::mm::KERNEL_HEAP_SIZE`） |
| 分配 API | `GlobalAlloc::alloc` / `realloc`（经 `Box`/`Vec`/`String`/`BTreeMap`/`Arc` 等隐式调用）；显式 `alloc::alloc::alloc_zeroed`（内核栈） |
| 回收 API | `GlobalAlloc::dealloc`（`Drop` 触发）；`drop_user_aspace` 等显式 `Box::from_raw` + `drop` |
| 初始化 | `runtime::heap_allocator::init()` — **必须在任何堆分配前调用一次**（`os/src/main.rs` 引导路径） |

> **文档漂移**：`docs/exports/features/wateros-runtime.md` 仍写「buddy 分配器 + panic」，实码已切换为 `linked_list_allocator`（`runtime-heap-allocator/Cargo.toml` `default = impl-linked-list-allocator`）。

---

## 2. 分配入口

### 2.1 分配器本体

| 入口 | 文件 | 说明 |
|------|------|------|
| `InterruptSafeLockedHeap::alloc` | `runtime-heap-allocator/src/lib.rs` | `#[global_allocator]`；分配前 `disable_global_interrupt`，防中断重入 |
| `heap_allocator::init` | 同上 | `LockedHeap::init(HEAP_SPACE, KERNEL_HEAP_SIZE)` |
| `handle_alloc_error` | 同上 | 由根 crate `#[alloc_error_handler]` 委托；**打印 layout 后 `panic!`** |
| 递归分配检测 | `with_allocator_interrupt_guard` | `HEAP_GUARD_DEPTH > 0` 时 **`panic!("recursive heap allocation")`** |

### 2.2 引导期固定消耗（堆初始化后、用户态之前）

| 路径 | 文件 | 近似堆占用 | 备注 |
|------|------|-----------|------|
| 帧分配器元数据 | `mm-frame-alloctor/.../impl-stack/src/lib.rs` `init()` | `allocated: Vec<bool>` + `ref_counts: Vec<usize>` ≈ **1 + 8 字节/可用帧** | QEMU `-m 1G` 下约 **2–3 MiB**；`mm.rs` 注释已说明；**永不收缩** |
| 帧分配器自测 | `test_with_range` | 临时 `Vec<PhysPageNum>` 批次 256 | 自测结束释放；但元数据 Vec 常驻 |
| 任务表 / 调度器 | `task-scheduler/.../registry.rs` 等 | `Vec<TaskSlot>`、`WaitQueues` | idle + 内核任务 TCB/`KernelStack` |
| 内核全局页表对象 | `mm-impl/.../kernel_global.rs` | `Box::leak(Box::new(aspace))` | **故意永久占用**（Sv39AddressSpace 结构 + VMA Vec 元数据，页表帧走帧池） |
| MM 自测 / ELF 加载 | `kernel_elf.rs` | 临时 `Vec`、路径 `String` | 加载完成后用户 aspace 转 `Box::leak` 直至 exit/execve |

引导顺序（RISC-V 路径，`os/src/main.rs`）：`heap_allocator::init()` → `task::init()` → `mm::test_with_range`（含 `init_frame_allocator`）→ `kernel_mm::init()` → 驱动/FS/VFS。

### 2.3 运行时主要堆消费者（元数据 / 缓冲）

| 子系统 | 主要结构 | 文件 | 单实例规模 | 上限 / 增长 |
|--------|---------|------|-----------|------------|
| 任务 TCB | `Box<TaskControlBlock>`、`KernelStack`（32 KiB） | `task-impl/impl-core/tcb.rs`、`task-api/.../kernel.rs` | 32 KiB + TCB | 与任务数同阶；槽位可复用 |
| 用户地址空间 | `Sv39AddressSpace`（VMA `Vec`） | `mm-impl/impl-sv39/src/pagetable.rs` | 数 KB + 按 mmap 增长 | 每进程 1 份；`Box::leak` 至 exit/execve |
| FD 表 | `Vec<Box<dyn VfsIoHandle>>`、标志/owner 表 | `vfs-impl/impl-fd-session/src/registry.rs` | 按 fd 数 | `RLIMIT_NOFILE` 1024 |
| 根卷页缓存 | `PageFrame { data: Vec<u8> }` × 4096 | `vfs-impl/impl-page-cache/src/lib.rs` | **≈ 16 MiB** 数据 + 索引 | `FILE_PAGE_CACHE_CAPACITY` 硬顶 |
| 块设备缓存 | `Slot { data: Vec<u8> }` × 64 | `driver-block/.../impl-block-cache/src/lib.rs` | 64 × block_size | 每包装设备一份 |
| 辅助卷小文件 | `BufferedFileHandle { data: Vec<u8> }` | `vfs-impl/impl-fs-bridge/src/file_handle.rs` | **整文件** | AuxRw/AuxRo 路径；根卷已改 `PagedFileHandle` |
| TmpFs | `TmpNode::File { data: Vec<u8> }`、`BTreeMap` 节点 | `vfs-impl/impl-fs-bridge/src/tmpfs.rs` | 文件大小 | unlink/rmdir 回收节点；inode 号单调不复用 |
| Pipe | `PipeState.buf: Vec<u8>` | `ipc-pipe/.../impl-ringbuf/src/kernel_pipe.rs` | **4096 B**（`DEFAULT_PIPE_CAPACITY`） | `Arc` 归零 Drop |
| TCP socket | smoltcp 缓冲 | `driver-network/src/lib.rs` | **256 KiB/socket**（`TCP_BUFFER_SIZE`） | 无显式 socket 数量上限 |
| Unix 域套接字 | `inbox: VecDeque<Vec<u8>>`、bind 表 | `syscall-impl/.../unix_sock.rs` | 按报文 | **队列无硬上限** |
| Futex | `BTreeMap<FutexKey, WaitQueue>` | `ipc-futex/.../impl-task/src/hub.rs` | 按 distinct key | 空队列可 `cleanup_empty_queue` 移除 |
| SysV SHM 注册表 | `BTreeMap`、附着 `Vec` | `ipc-shm/src/lib.rs` | 元数据；数据在帧池 | 单段最大 4 MiB |
| Syscall 临时缓冲 | `alloc::vec![0u8; len]` | `sys/read.rs`（4 MiB cap）、`sys/recvfrom.rs`（**无 cap**）等 | 按 syscall | 见 §5 |
| 网络协议栈 | `NetworkStack`、`SocketSet` | `driver-network/src/lib.rs` | 启动时一次性 | `init()` 分配 |
| PagedFileHandle detached | `detached_data: Vec<u8>` | `paged_handle.rs` | 可随写扩展 | close 时 Drop |

---

## 3. 回收入口

| 场景 | 入口 | 是否归还堆 |
|------|------|-----------|
| `Box`/`Vec`/`String`/`Arc` Drop | 编译器生成 → `GlobalAlloc::dealloc` | 是 |
| 任务 reap | `TaskRegistry::remove` → TCB Drop → `KernelStack` Drop | 是（32 KiB/任务） |
| 进程/线程 exit | `drop_user_aspace_on_task_exit` → `Box::from_raw` + `Sv39AddressSpace::destroy` | 是（aspace 对象 + VMA Vec；用户页走帧池） |
| execve | `mm::kernel_mm::drop_user_aspace(old_aspace)` | 是 |
| `close(fd)` | `VfsIoHandle::close` / fd 表 `close_slot` → `Box<dyn VfsIoHandle>` Drop | 是 |
| pipe 两端关闭 | `Arc<Pipe>` 引用归零 | 是（含 4 KiB 环缓冲） |
| tmpfs unlink/rmdir | `children.remove` → `TmpNode` Drop | 是 |
| 页缓存 LRU | 槽位复用，不释放 `Vec<u8>` 容量 | **否**（固定 16 MiB 常驻） |
| 帧分配器元数据 | 无 API | **否** |
| 内核全局 aspace | 无 | **否**（故意 leak） |
| futex 空队列 | `cleanup_empty_queue` | 是（`WaitQueue` 节点） |
| unix_sock unregister | `FD_TABLE.remove`；bind 表项移除 | 部分（socket 对象）；**已入队报文依赖消费** |

---

## 4. 生命周期状态机

```
[未初始化]
    │ heap_allocator::init()
    ▼
[堆就绪 · 全空闲 128MiB]
    │ GlobalAlloc::alloc / Box::new / Vec::push ...
    ▼
[已分配 · 使用中] ──Drop/dealloc──► [已释放 · 回空闲链表]
    │                                      │
    │ OOM（分配器返回 null）                │ 可再次 alloc
    ▼                                      │
[alloc_error_handler → panic（内核停机）]     │
    │                                      │
    │ Box::leak（kernel/user aspace）       │
    ▼                                      │
[永久占用 · 无自动回收] ──exit/execve/drop_user_aspace──► [已释放]
```

**半初始化风险**：

- `fork_user_aspace` 成功（`Box::into_raw`）后，若 `fork_current` 返回 `None`，子 aspace **无任务持有、无 drop 钩子** → 堆对象 + 页表帧泄漏（见 §6 P1）。
- `KernelStack::new`：`alloc_zeroed` 失败返回 null 后仍 `Box::from_raw(ptr)` → **未定义行为**（见 §6 P0）。
- 页缓存 `global_cache` 重建：`mount_gen` 递增时新建 `GlobalFilePageCache`（再占 ≈16 MiB），旧 `Arc` 待所有 `PagedFileHandle` 关闭才释放 → 短暂双份占用尖峰。

---

## 5. 账本稳定性结论

**评级：部分稳定**

| 维度 | 结论 |
|------|------|
| 常规 Drop 配对 | `linked_list_allocator` 非侵入式空闲链表，正常路径 alloc/dealloc 成对；与历史 buddy UAF 破坏空闲链表问题相比更安全 |
| 故意永久占用 | 内核全局 `Sv39AddressSpace`、`HEAP_SPACE` 本体 — 设计如此，需计入预算 |
| 错误路径回滚 | **不完整**：clone fork 失败未 `drop_user_aspace`；部分 syscall 大缓冲 OOM 直接 panic 而非 `ENOMEM` |
| 引用计数 | `Arc`（pipe、socket、页缓存 entry）语义正确；fd 表 `ref_counts` 在 vfs 层单独维护 |
| double-free / UAF | 分配器有中断屏蔽 + 递归检测；未见明显 double-free 路径；**OOM 时 null 指针未检查**为 UB 风险 |
| 无运行期账本 API | **无** `heap_used` / `heap_free` 统计；`frame_mem_stats` 仅覆盖帧池，不覆盖堆 |

---

## 6. 耗尽与失败处理

| 场景 | 当前行为 | 与 Linux/预期差距 |
|------|---------|------------------|
| 通用 `Box::new` / `Vec::resize` 失败 | `alloc_error_handler` → **`panic!`** | Linux 内核应返回 `ENOMEM` 或走可恢复错误，不应整机 panic |
| `KernelStack::new` OOM | null → `Box::from_raw` | 应返回 `Err` 或 panic 于显式检查点，禁止 UB |
| `read`/`write` syscall | `len > 4 MiB` → 收敛错误 | 合理 |
| `sendto` | `len > 65536` → `EINVAL` | 合理 |
| `recvfrom` | **`alloc::vec![0u8; len]` 无上限** | 用户 `len` 极大 → OOM panic；应同 sendto 限 64KiB 或返回 `ENOMEM` |
| `fork_user_aspace` / `fork_cow` | 帧不足 → `MmError::OutOfMemory` → syscall `ENOMEM` | 合理（帧池与堆分离） |
| `clone` 任务创建失败 | `EAGAIN`，**不回收已分配 aspace** | 应 `drop_user_aspace(new_aspace_ptr)` |
| 帧分配器元数据 resize | 引导期一次性；失败则 panic | 大内存机器上占 2–3 MiB 永久堆，压缩可用余量 |
| 递归堆分配（如分配中打 log 再分配） | `panic!` | 日志路径应避免堆分配 |

**有效堆预算粗算**（QEMU `-m 1G` 典型 bring-up）：

```
128 MiB（总量）
−  2~3 MiB（帧分配器元数据，常驻）
− 16 MiB（页缓存预分配，首次 open 根卷文件时）
−  0.5~2 MiB（块缓存 × 设备数）
− 32 KiB × 任务数（内核栈）
− 256 KiB × TCP socket 数
− 动态：TCB、fd 表、VMA、tmpfs、unix 队列、syscall 临时缓冲……
≈ 100 MiB 量级可用（未计用户 aspace 元数据与大文件缓冲）
```

`base-config/fs.rs` 注释仍写「内核堆共 64MiB」，与当前 `KERNEL_HEAP_SIZE = 128 MiB` **不一致**，维护时应同步。

---

## 7. 跨资源耦合

| 耦合 | 说明 |
|------|------|
| 堆 ↔ 物理帧池 | 帧分配器 **元数据 Vec 在堆上**；页缓存 **数据缓冲在堆、页帧在帧池**；SHM 段页表在堆注册、数据在帧池 |
| fork / exit | `fork_user_aspace` 堆分配 aspace + 帧池 COW；`exit` 必须同时 `drop_user_aspace` + reap TCB 才闭环 |
| execve | 旧 aspace 堆回收 + 新 ELF `Box::leak` 新 aspace |
| fd / VFS | `Box<dyn VfsIoHandle>` 在堆；close 释放；fork dup 增加 `Arc`/句柄引用 |
| 页缓存 remount | `bump_mount_generation` → 新缓存 16 MiB 堆分配；旧缓存与打开句柄耦合 |
| 锁 | `InterruptSafeLockedHeap` 内 `LockedHeap` 使用 `spin::Mutex`；分配时关中断（见 `lock-inventory.md` #9） |
| 引导顺序 | **必须先** `heap_allocator::init` 再 `init_frame_allocator`（否则 `Vec::resize` 无堆） |

---

## 8. 潜在问题列表

### P0（泄漏 / UAF / 卡死 / 雪崩）

| ID | 类型 | 描述 | 位置 |
|----|------|------|------|
| KH-P0-1 | 卡死/雪崩 | **任意堆 OOM → 全局 `panic!`**，长测后期元数据累积触发分配失败即整机停机，表现为「后期异常/卡死」 | `runtime-heap-allocator/src/lib.rs` `handle_alloc_error`；`os/src/main.rs` `alloc_error_handler` |
| KH-P0-2 | UAF/UB | `KernelStack::new` 不检查 `alloc_zeroed` 返回值；OOM 时 `Box::from_raw(null)` | `task-api/api-v0/src/kernel.rs:42-43` |
| KH-P0-3 | 雪崩 | `sys_recvfrom`（及同类路径）按用户 `len` **无上限**分配 `kbuf`，可触发巨型分配 → OOM panic；`sendto` 已有 64KiB 限制而不一致 | `syscall-impl/.../sys/recvfrom.rs:37,57,70` |
| KH-P0-4 | 泄漏 | `clone`/`fork`：`fork_user_aspace` 成功后若 `fork_current` 返回 `None`，**未调用 `drop_user_aspace(new_aspace_ptr)`**，每次失败泄漏 aspace 堆对象（含 VMA Vec）及子页表帧 | `syscall-impl/.../sys/clone.rs:175-188` |

### P1（错误路径回滚 / 限额）

| ID | 类型 | 描述 | 位置 |
|----|------|------|------|
| KH-P1-1 | 尖峰耗尽 | 页缓存 `mount_gen` 重建时新旧 `GlobalFilePageCache` 短暂共存，堆尖峰 ≈32 MiB | `impl-page-cache/src/lib.rs` `global_cache` / `reset_global_cache` |
| KH-P1-2 | 无上限增长 | `unix_sock` `inbox` / `accept_queue` / `dgram_inbox` 为无界 `VecDeque`，恶意或高压测试可持续吃堆 | `unix_sock.rs` |
| KH-P1-3 | 无上限增长 | `FutexHub.queues` 按 distinct key 增长，无全局条目上限 | `ipc-futex/.../hub.rs` |
| KH-P1-4 | 大缓冲 | 辅助卷 `BufferedFileHandle` 仍整文件读入堆；多开大文件可快速耗尽 | `file_handle.rs`（`FsRoute::AuxRw/AuxRo`） |
| KH-P1-5 | 配置漂移 | `FILE_PAGE_CACHE_CAPACITY` 注释写 64MiB 堆，实际 128MiB；误导容量规划 | `base-config/src/fs.rs:12-13` |
| KH-P1-6 | 可观测性 | 无堆用量统计 / warn 阈值，耗尽前无法诊断 | 全库缺失 |

### P2（语义 / 文档）

| ID | 类型 | 描述 |
|----|------|------|
| KH-P2-1 | 文档 | `wateros-runtime.md` 仍描述 buddy 分配器 |
| KH-P2-2 | 设计 | 内核全局 aspace、`HEAP_SPACE` 静态 128MiB 为故意不回收，应在资源手册中标注 |
| KH-P2-3 | 递归 panic | 分配路径中若触发二次堆分配（如格式化日志），`recursive heap allocation` panic |

---

## 9. 收敛建议

1. **OOM 策略分层**：引导期 / 不可恢复路径可保留 panic；**syscall 与任务创建**路径应优先返回 `ENOMEM`/`EAGAIN`，禁止 silent 继续。
2. **`KernelStack::new`**：`alloc_zeroed` 失败时 `panic!` 于显式检查点（或返回 `Result` 向上传播为 `EAGAIN`），禁止 `from_raw(null)`。
3. **统一 syscall 缓冲上限**：`recvfrom`/`recvmsg` 与 `sendto`/`sendmsg` 对齐（建议 **64 KiB** 或 `MIN(len, 64KiB)` 分块读），超限返回 `EINVAL` 或 `ENOMEM`。
4. **clone 回滚**：`fork_user_aspace` 成功后注册 `scopeguard` 或在 `fork_current == None` 分支调用 `drop_user_aspace(new_aspace_ptr)`。
5. **堆水位 warn**：在 `GlobalAlloc::alloc` 失败前（或周期性）打印 `used/free/capacity`；接近 90% 时 `warn!` 含调用栈线索（资源名、操作、文件）。
6. **无界结构加 cap**：`unix_sock` 队列、`FutexHub` 表项设硬上限，耗尽返回 `EAGAIN`/`ENOMEM` 并 warn。
7. **辅助卷 I/O**：Aux 路径评估迁移 `PagedFileHandle` 或 `read_range`，避免 `BufferedFileHandle` 整文件堆占用。

---

## 10. 修复任务草案

| 优先级 | 标题 | 主要文件 | 验收标准 |
|--------|------|---------|---------|
| P0 | clone fork 失败回滚子 aspace | `sys/clone.rs` | `fork_current == None` 或后续 setup 失败时调用 `drop_user_aspace`；fork 压力测试无 aspace 泄漏 |
| P0 | 修复 KernelStack OOM UB | `task-api/api-v0/src/kernel.rs` | `alloc_zeroed == null` 时不调用 `from_raw`；任务创建失败返回 `EAGAIN` |
| P0 | recvfrom 缓冲上限 | `sys/recvfrom.rs` | `len > 65536` 返回 `EINVAL`；与 `sendto` 一致；LTP recv 相关用例通过 |
| P1 | 堆用量统计 + OOM warn | `runtime-heap-allocator/src/lib.rs` | 提供 `heap_mem_stats()`；分配失败前若可检测则 warn `used/cap` |
| P1 | unix_sock 队列上限 | `unix_sock.rs` | 单 socket inbox/accept 超限时 drop 或返回 `EAGAIN` 并 warn |
| P1 | 同步配置注释 | `base-config/fs.rs`、`wateros-runtime.md` | 堆大小、分配器类型与代码一致 |
| P2 | 评估 syscall 路径 FallibleAlloc | 各 `sys/*.rs` | 关键路径 `try_reserve` 失败映射 `ENOMEM` 而非全局 panic |

---

## 11. 与 syscall / 锁审计交叉项

- **clone + fd 表**：fd 继承在 `fork_current` 之后；若修复 KH-P0-4，需确认未遗留半初始化子任务 fd 表（见 `file-descriptors` 分组）。
- **堆分配器锁**：`InterruptSafeLockedHeap` 持 `LockedHeap` 内 `Mutex` 且关中断；与 `lock-inventory.md` #9 一致；多核扩展需重新评估。
- **read/write 4MiB 上限**：已收敛；recvfrom 未收敛（KH-P0-3）。

---

## 12. 审计结论摘要

内核堆以 **128 MiB 静态区域 + linked-list 分配器** 服务全内核元数据；正常 `Drop` 路径账本**基本可靠**，但 **OOM 一律 panic**、**KernelStack null 未检查**、**recvfrom 无缓冲上限**、**fork 失败不回滚 aspace** 四条路径可在压力/长测下引发 **整机停机或隐性泄漏**，与「测试后期卡死/异常」背景高度相关。帧分配器元数据（≈2–3 MiB）与页缓存（16 MiB）为启动后**永久或准永久**堆占用，须在容量规划中扣除。

**账本稳定性：部分稳定**
