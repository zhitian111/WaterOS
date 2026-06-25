# driver-slots 资源生命周期审计

> **分组 ID**：`driver-slots`  
> **覆盖资源**：#36–41（块/字符/网络设备注册槽、VirtIO DMA 物理页、PCI MMIO BAR、klog 环形缓冲）  
> **审计时间**：2026-06-25  
> **搜索范围**：`os/components/wateros-driver/**`、`os/components/wateros-klog/**`  
> **Baseline**：单核多线程；对照 Linux 设备模型（注册后常驻、无热拔）与常见 errno 语义

---

## 总览

| # | 资源 | 主要类型 | 账本稳定性 | 硬上限 |
|---|------|---------|-----------|--------|
| 36 | 块设备注册槽 | `BLOCK_DEVICES: Mutex<Vec<SharedBlockDevice>>` | **部分稳定** | 无 |
| 37 | 字符设备注册槽 | `CHARACTER_DEVICES: Mutex<Vec<SharedCharacterDevice>>` | **部分稳定** | 无 |
| 38 | 网络设备注册槽 | `NETWORK_DEVICES: Mutex<Vec<SharedNetworkDevice>>` | **部分稳定** | 无 |
| 39 | VirtIO DMA 物理页 | HAL `dma_alloc` / `dma_dealloc` | **部分稳定** | 受全局帧池约束 |
| 40 | PCI MMIO BAR | `VirtioPciBarAllocator` / `VirtioNetPciBarAllocator` | **不可靠**（仅 bump） | 窗口约 1GiB（分块/网卡子区间） |
| 41 | Klog 环形缓冲 | `KlogRingbufInner` | **稳定** | 256 槽 × 1024 B/条 |

**跨组件耦合**：设备注册 → `devfs::refresh()` 克隆 `Arc` 绑定路径；块设备 → 根 FS 挂载 / `CachingBlockDevice`；网络设备 → `network::stack::init` → `SmoltcpAdapter` 再持有一份 `Arc`；VirtIO DMA → `wateros-mm` 帧分配器。

---

## #36 块设备注册槽

### 资源标识

- **组件**：`wateros-driver/driver-block/block-api/api-v0`
- **类型**：`SharedBlockDevice = Arc<Mutex<Box<dyn BlockDevice>>>`
- **全局表**：`static BLOCK_DEVICES: Mutex<Vec<SharedBlockDevice>>`

### 分配入口

| 入口 | 文件 | 条件 |
|------|------|------|
| `register_block_device(device)` | `driver-block/block-api/api-v0/src/lib.rs` | 任意时刻 `push` 到全局 `Vec` |
| DTB virtio-mmio 探测 | `driver-impl/impl-qemu-riscv64-opensbi/src/lib.rs` → `probe_virtio_blk_and_collect_unsupported` | `device_id==2`，`VirtioBlkDevice::from_mmio` 成功 |
| PCI virtio-blk 探测 | `driver-impl/impl-qemu-loongarch64-virt/src/lib.rs` + `pci.rs` | `probe_first_from_ecam` 找到首块盘 |
| 块缓存包装 | `impl-block-cache/manager.rs` → `BlockCacheManager::wrap` | RISC-V feature `impl-block-cache` 启用时 |

典型启动链：`main` → `driver::active_impl::init_after_boot()` → 平台 `init_after_boot` → `register_block_device`。

### 回收入口

- **无** `unregister_block_device` 或表清空 API。
- 设备随 `Arc` 引用计数释放；全局 `Vec` 持有强引用 → **注册后进程生命周期内不释放**。
- `devfs::refresh()` 仅重建路径绑定，不删除全局表项。

### 生命周期状态机

```
未注册 → register_block_device → 已注册（Vec 持有 Arc）
                                      ↓
                            devfs / FS / 块缓存 克隆 Arc
                                      ↓
                            内核运行期常驻（无「已注销」状态）
```

**半初始化**：`VirtioBlkDevice::from_mmio` 失败时不注册（`probe_virtio_blk` 记入 `unsupported`）；成功后才 `register_block_device`。PCI 路径若 `assign_memory_bars` 部分成功但整体 `from_pci_root` 失败，BAR 空间已消耗但块设备未入表（见 #40）。

### 账本稳定性

| 维度 | 结论 |
|------|------|
| 分配/释放成对 | 注册侧无释放；依赖「一次性 boot 注册」假设 |
| 引用计数 | `Arc` 正确；`devfs` `block_bindings` 与全局表共享同一 `Arc` |
| double-free / UAF | 未见显式路径 |
| 泄漏 | **重复 `init_after_boot` 会追加条目**（见问题 D1） |

### 耗尽处理

- `Vec` 无容量上限；耗尽时依赖堆分配失败（可能 panic）。
- 无 `ENOMEM` 式错误码；`register_block_device` 不返回 `Result`。

### 跨资源耦合

- 注册后 `devfs_impl::refresh()` 生成 `/dev/vblk{N}`、`/dev/vd*`。
- `CachingBlockDevice` 预分配 64×512B 堆缓冲（`BLOCK_CACHE_CAPACITY_BLOCKS`），与块缓存审计（#19）交叉。
- 根 FS `mount_root_*_from_block_path` 使用 `first_block_device()` 或 devfs 默认路径。

---

## #37 字符设备注册槽

### 资源标识

- **组件**：`wateros-driver/driver-character/character-api/api-v0`
- **类型**：`SharedCharacterDevice = Arc<Mutex<Box<dyn CharacterDevice>>>`
- **全局表**：`static CHARACTER_DEVICES: Mutex<Vec<SharedCharacterDevice>>`

### 分配入口

| 入口 | 文件 | 说明 |
|------|------|------|
| `register_character_device` | `character-api/api-v0/src/lib.rs` | 追加到 `Vec` |
| DTB UART | `impl-qemu-riscv64-opensbi` → `probe_character_devices` | 每个匹配 `ns16550a`/`ns8250` 节点 |
| UART 回退 | 同上，`character_device_count()==0` | 固定 QEMU virt UART0 |
| 内置 stub | `register_builtin_character_devices` | RTC / null（feature 控制） |
| LoongArch | `impl-qemu-loongarch64-virt` | 仅 `register_builtin_character_devices` + 后期 `uart::init_default_virt_uart` |

### 回收入口

- 无 unregister；与块设备相同，**常驻至内核结束**。

### 生命周期状态机

与 #36 同构：`未注册 → register → 常驻 →（无注销）`。

### 账本稳定性

- **部分稳定**：RISC-V 可对 DTB 中多个 UART 各注册一条；回退逻辑避免零设备。
- 重复 `init_after_boot` 会重复注册 UART + builtin（D1）。

### 耗尽处理

- 无上限；典型 1–3 个设备。

### 跨资源耦合

- `devfs::refresh` 绑定 `/dev/ttyS{N}`、`/dev/console`、`/dev/null` 等。
- VFS `char_dev_handle` 通过 devfs 路径解析到 `SharedCharacterDevice`。
- syscall `read`/`write`/`ioctl` 经 fd 会话持锁访问设备。

---

## #38 网络设备注册槽

### 资源标识

- **组件**：`wateros-driver/driver-network/network-api/api-v0`
- **类型**：`SharedNetworkDevice = Arc<Mutex<Box<dyn NetworkDevice>>>`

### 分配入口

| 入口 | 文件 |
|------|------|
| `register_network_device` | `network-api/api-v0/src/lib.rs` |
| DTB virtio-net mmio | `impl-qemu-riscv64-opensbi` |
| PCI virtio-net | `impl-qemu-loongarch64-virt` + `pci.rs` |

### 回收入口

- 无 unregister；常驻。

### 生命周期状态机

```
未注册 → register_network_device → 常驻
                ↓
    network::stack::init → SmoltcpAdapter::new(device) 克隆 Arc
                ↓
    create_tcp_socket / create_udp_socket → 堆上 smoltcp 缓冲（属 sockets 分组 #27）
```

### 账本稳定性

- **部分稳定**；`NETWORK_STACK` 与全局表各持 `Arc`，计数正确。
- 网络设备**不**进入 devfs（无 `/dev/eth0` 类节点）；用户态经 socket syscall 间接使用。

### 耗尽处理

- 无设备数量上限。
- `network::stack::init` 无网卡时降级 `loopback_only()`，不失败。

### 跨资源耦合

- 与 **sockets** 分组强耦合：`driver-network/src/lib.rs` `stack` 模块。
- VirtIO net DMA 见 #39。

---

## #39 VirtIO DMA 物理页

### 资源标识

- **实现位置**（四套 HAL，逻辑同构）：
  - `driver-block/block-impl/impl-virtio-mmio/src/lib.rs` — `VirtioMmioHal`
  - `driver-block/block-impl/impl-virtio-pci/src/lib.rs` — `VirtioPciHal`
  - `driver-network/network-impl/impl-virtio-mmio/src/lib.rs`
  - `driver-network/network-impl/impl-virtio-pci/src/lib.rs`
- **底层 API**：`frame_alloctor::{frame_alloc_result, frame_dealloc_result}`

### 分配入口

- `virtio-drivers` 经 `Hal::dma_alloc(pages, direction)` 分配 **物理连续、页对齐、已清零** 内存。
- 恒等映射：`paddr == vaddr`（`usize` 视图）。
- 多页路径：循环 `frame_alloc_result`，失败时回滚已分配页；校验栈式分配器给出的 **PPN 递减连续** 性。

### 回收入口

- `Hal::dma_dealloc` → 按 `base_ppn` 循环 `frame_dealloc_result`。
- `virtio-drivers` 的 `Dma<H>` 在 `Drop` 时调用 `dma_dealloc`（失败则 `assert_eq!(err, 0)`）。
- `VirtIOBlk`/`VirtIONet` 的 `Drop` 主要 `queue_unset`；DMA 页由内部 `Dma` 成员释放。

### 生命周期状态机

```
帧池未占用 → dma_alloc（VirtIO 设备/队列初始化）
                ↓
         设备运行期（队列、描述符环）
                ↓
    Drop VirtIO 设备 / Dma → dma_dealloc → 帧回池
```

**常驻路径**：设备一旦 `register_*` 进入全局表且不 Drop → **DMA 页永久占用帧池**（与 Linux 驱动加载后常驻一致）。

### 账本稳定性

| 维度 | 结论 |
|------|------|
| 单次 alloc 回滚 | **稳定**（partial alloc 已释放） |
| dealloc 错误处理 | `frame_dealloc_result` 返回值被 `_` 忽略；`virtio-drivers` 侧 assert |
| 设备未 Drop | 注册表持 `Arc` → DMA 不回收 → **帧池单调减少** |
| 重复注册同一硬件 | 新 `Virtio*Device::new` 再占 DMA，旧实例仍在 Vec → **泄漏式累积**（D1） |

### 耗尽处理

- `dma_alloc` OOM：记录 `error!` 日志，返回 `(0, NonNull::dangling())` → `virtio-drivers` 返回 `Error::DmaError` → 设备初始化失败，**不注册**。
- 非连续页：同样失败并回滚。
- **无**驱动级 DMA 页配额或 `used/total` 预警。

### 跨资源耦合

- 与 **physical-frames**（#1）共享 `StackFrameAllocator`。
- 块缓存、页缓存、用户 mmap 竞争同一帧池；多 virtio 设备时风险上升。

---

## #40 PCI MMIO BAR 地址

### 资源标识

- **类型**：`VirtioPciBarAllocator` / `VirtioNetPciBarAllocator`（`next`/`end` bump 指针）
- **窗口**（LoongArch `pci.rs`）：
  - 块设备：`0x4000_0000..0x8000_0000`
  - 网卡：`0x5000_0000..0x8000_0000`（子区间）

### 分配入口

- `assign_memory_bars` → `allocator.allocate(size)`：按 `size` 对齐 bump。
- 成功则 `root.set_bar_32/64` 写入 PCI 配置空间。

### 回收入口

- **无**；`allocate` 仅前进 `next`，永不回收。
- 设备销毁或 probe 失败不回滚已分配 BAR 虚拟地址区间。

### 生命周期状态机

```
[bump start] → allocate(BAR0) → allocate(BAR1) → … → 窗口耗尽 → allocate 返回 None
```

### 账本稳定性

- **不可靠**（有意为之的 boot 期 bump）；当前仅枚举 **首个** virtio-blk/net，窗口足够。
- **部分 BAR 已写、后续失败**：`from_pci_root` 返回 `Err` 时 BAR 分配器已消耗空间，PCI 配置可能处于中间态（D3）。

### 耗尽处理

- `allocate` 返回 `None` → `logging::warn!` + `DriverError::Unsupported`。
- Below-1MiB BAR → 直接 `Unsupported`。

### 跨资源耦合

- 与 MMIO 恒等映射、PCI ECAM 枚举（`impl-qemu-loongarch64-virt`）绑定。
- 与 #36/#38 设备注册：BAR 成功是 VirtIO PCI transport 前置条件。

---

## #41 Klog 环形缓冲

### 资源标识

- **组件**：`wateros-klog/klog-impl/klog-ringbuf`
- **类型**：`KlogRingbufInner`（`slots: [Slot; KLOG_DESC_SLOTS]`）
- **配置**（`wateros-base-config/klog.rs`）：
  - `KLOG_DESC_SLOTS = 256`
  - `KLOG_MAX_RECORD_BYTES = 1024`
  - `KLOG_TEXT_RING_BYTES = 32768`（**仅用于 `buffer_bytes()` 返回值，非实际存储布局**）

### 分配入口

| 入口 | 说明 |
|------|------|
| `KlogRingbuf::init()` / 惰性 `ensure_inner` | 首次写入时 `default()` |
| `KlogRingbufInner::append` | `record` / `klog_*!` 宏 / `syslog` 写优先级 |
| 宏 `KlogFmtBuffer` | 栈上 512B 格式化后 `record` |

### 回收入口

- **覆盖式回收**：`count == KLOG_DESC_SLOTS` 时覆写 `head` 槽，`records_dropped++`。
- `read_cursor` 随丢弃推进，避免读已覆盖序号。
- `KlogRingbuf::init()` / `reset()` 清空全环（`init()` 可重复调用）。
- `SYSLOG_ACTION_CLEAR` → `clear_read_cursor()`（不清空存储，仅标记已读）。

### 生命周期状态机

```
空环 → append（count++）→ 满 256 槽 → 覆写最旧槽（逻辑释放 + dropped 计数）
         ↓
    peek / advance_read_cursor（消费者）
```

### 账本稳定性

- **稳定**：槽位与 `seq` 单调递增；覆写时更新 `oldest_seq` / `read_cursor`。
- 无 double-free；静态 `slots` 数组无堆分配。
- **文档/实现偏差**：架构文档描述「desc + text 字节环」，实现为 **每槽固定 1024B 数组**，`KLOG_TEXT_RING_BYTES` 未参与存储（D4）。

### 耗尽处理

- 槽满：**静默覆写**最旧记录（`records_dropped` 可观测），符合环形缓冲预期。
- 单条超长：截断至 `KLOG_MAX_RECORD_BYTES`，置 `TRUNC` 标志。
- 宏格式化：超过 512B **静默截断**（无 `TRUNC`）。
- 未知 `syslog` action → **panic**（D5）。

### 跨资源耦合

- `sys_syslog` syscall → `wateros-klog/src/syscall.rs`。
- 写路径关全局中断（`KlogInterruptGuard`），与锁审计交叉。
- 与内核堆无关（除 syscall 格式化栈缓冲）。

---

## 潜在问题列表

| ID | 严重度 | 类型 | 描述 |
|----|--------|------|------|
| **D1** | **P0** | 泄漏 / 状态不一致 | 三类 `register_*` 全局表**无幂等、无清空**；`impl-qemu-*::test()` 内再次调用 `init_after_boot()` 会**追加**设备条目。旧 `Virtio*Device` 仍被 `Vec` 持有，新实例再次 `dma_alloc` 并绑定同一 MMIO/PCI 硬件 → DMA 帧与驱动状态累积。主线 `main` 当前仅 boot 调用一次，但 `driver::test()` 已暴露该路径。 |
| **D2** | **P0** | 静默耗尽 / 雪崩 | 已注册 VirtIO 设备的 DMA 页**永不释放**（设计如此），且无全局配额；多 DTB 节点或叠加页缓存/块缓存时帧池耗尽，后续 `dma_alloc`/页故障路径失败，表现为后期随机 `ENOMEM` 或卡死。 |
| **D3** | P1 | 泄漏（BAR） | PCI `assign_memory_bars` 部分成功后 `from_pci_root` 失败，bump 分配器不回收，PCI 配置可能半初始化。 |
| **D4** | P1 | 语义不符 | `KlogRingbufInner::buffer_bytes()` 返回 `KLOG_TEXT_RING_BYTES`（32KiB），实际容量为 `256×1024` 静态槽；`SYSLOG_ACTION_SIZE_BUFFER` 可能误导用户态缓冲 sizing。 |
| **D5** | P1 | panic | `dispatch_kernel` 对未知 `syslog` action `panic!`；恶意或错误 syscall 可击垮内核。 |
| **D6** | P1 | 错误路径 | `dma_dealloc` 忽略 `frame_dealloc_result` 错误；若帧账本异常，`virtio-drivers` `Dma::drop` 可能 `assert` panic。 |
| **D7** | P2 | 无上限 | 设备 `Vec` 无 `MAX_*` 常量；异常 DTB 大量节点可导致堆增长。 |
| **D8** | P2 | 静默截断 | `klog_*!` 宏经 512B 栈缓冲，无 `TRUNC` 标志。 |
| **D9** | P2 | 交叉 | 网络设备不注册 devfs；与 socket 资源（#27–29）生命周期分离，排查泄漏时需跨分组。 |

---

## 收敛建议

### 设备注册表（#36–38）

1. **幂等 boot**：`init_after_boot` 入口增加 `static INIT_DONE` 或「清空后重扫」策略；禁止盲目 `push`。
2. **`driver::test()`**：改为只读探测（`block_device_count`、读块 0 用**已注册**句柄），**禁止**再次完整 `init_after_boot`。
3. 长期：提供 `unregister_*` 或 `replace_at(index)` 供热拔/重绑；短期至少 `warn!` 打印 `used/capacity`（Vec len）。
4. 可选硬上限：`MAX_BLOCK_DEVICES` 等，耗尽返回 `DriverError::NoMemory` 并 `warn!`。

### VirtIO DMA（#39）

1. 在 `dma_alloc` OOM 路径增加 `warn!` 含 `pages` 与当前帧池用量（需 mm API 暴露统计）。
2. `dma_dealloc` 对 `frame_dealloc_result` 失败打 `warn!` 并返回非 0，避免静默账本损坏。
3. 文档化「每设备常驻 DMA 页」与帧池预算关系。

### PCI BAR（#40）

1. probe 失败路径记录已消耗 BAR 区间；多设备场景需分配器可回滚或按 device_function 隔离。
2. 窗口余量 `< threshold` 时 `warn!`。

### Klog（#41）

1. `buffer_bytes()` 改为返回 `KLOG_DESC_SLOTS * KLOG_MAX_RECORD_BYTES` 或真实占用统计。
2. 未知 syslog action 返回 `-EINVAL` 而非 panic。
3. 宏缓冲截断时设置 `TRUNC` 或 `warn!`（低频）。

---

## 修复任务草案

| 优先级 | 标题 | 文件 | 验收标准 |
|--------|------|------|----------|
| P0 | 设备注册幂等化 | `driver-impl/impl-qemu-riscv64-opensbi/src/lib.rs`、`impl-qemu-loongarch64-virt/src/lib.rs` | 连续两次 `init_after_boot` 设备计数不变；`driver::test` 不重复注册 |
| P0 | VirtIO DMA 帧池预警 | `impl-virtio-mmio`/`impl-virtio-pci` HAL | OOM 时 `warn!` 含 `pages`；文档记录每设备典型 DMA 页数 |
| P1 | PCI BAR 失败回滚/日志 | `impl-virtio-pci` `assign_memory_bars` | 失败路径日志含已分配 BAR 范围；单测或注释说明 bump 策略 |
| P1 | klog `buffer_bytes` 语义修正 | `klog-ringbuf/src/lib.rs` | `SYSLOG_ACTION_SIZE_BUFFER` 与真实可存字节一致 |
| P1 | syslog 未知 action 收敛 | `wateros-klog/src/syscall.rs` | 返回负 errno，不 panic |
| P2 | 设备表硬上限 | 三个 `api-v0/src/lib.rs` | 超限时 `warn!` + 拒绝注册 |
| P2 | `dma_dealloc` 错误传播 | 四套 VirtIO HAL | dealloc 失败可观测，不静默 |

---

## 账本稳定性总结

| 资源 | 结论 | 说明 |
|------|------|------|
| 块/字符/网络注册槽 | **部分稳定** | 单次 boot 路径正确；缺注销与幂等 |
| VirtIO DMA | **部分稳定** | alloc 回滚完善；常驻 + 重复注册风险 |
| PCI BAR | **不可靠** | 有意 bump；仅适合单设备 bring-up |
| Klog | **稳定** | 覆写语义清晰；配置常量与实现不一致 |

---

## 与 Linux / 预期语义差距

| 维度 | Linux 常见语义 | 当前实现 |
|------|---------------|----------|
| 设备注销 | `unregister_chrdev` / 驱动 remove | 无 |
| 设备数量 | 动态但受 udev/驱动模型管理 | 无限 `Vec` |
| DMA | `dma_alloc_coherent` + `dma_free` | 帧池 + 常驻 |
| PCI BAR | 固件或内核分配器管理 | 固定窗口 bump |
| klog | 环形缓冲覆写 + `dmesg` | 类似，但 `SIZE_BUFFER` 不准 |
| 耗尽 | `-ENOMEM` / 明确失败 | 设备注册不返回错误；DMA OOM 导致初始化失败 |

---

## 交叉引用

- 块设备缓存槽：[`block-cache.md`](block-cache.md)（#19）
- Inet socket / smoltcp：[`sockets.md`](sockets.md)（#27–29）
- 物理页帧池：[`physical-frames.md`](physical-frames.md)（#1）
- DevFS 节点刷新：[`fs-instances.md`](fs-instances.md)（#35）
- 锁：`KlogRingbuf::with` 持 `Mutex` + 关中断 — 见 `lock-inventory.md`
