# wateros-driver-block

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

`wateros-driver-block` 是 WaterOS 的块设备子系统。它只抽象“逻辑块寻址 + 主动读写”，不
实现文件系统、分区或调度语义。文件系统层通过 `BlockDevice` 按 LBA 读写扇区，是否支持写入
由具体设备决定；可选的块缓存层在驱动之上提供写穿 LRU 加速。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 按 feature 再导出块 API 与具体实现，提供 `supported_devices()`、`block_subsystem_claims_device()` 与 `test()`。 |
| 块设备 API | `block-api/api-v0/` | `Lba`、`BlockDevice` 与全局注册表，`BLOCK_SIZE` 固定为 512。 |
| VirtIO MMIO 实现 | `block-impl/impl-virtio-mmio/` | RISC-V QEMU `virtio-blk-device`。 |
| VirtIO PCI 实现 | `block-impl/impl-virtio-pci/` | LoongArch QEMU `virtio-blk-pci`。 |
| 块缓存实现 | `block-impl/impl-block-cache/` | `CachingBlockDevice` 写穿 LRU 缓存，对上仍实现 `BlockDevice`。 |
| 占位实现 | `block-impl/impl-dummy/` | 无硬件占位。 |

## 实现说明

- 块 API 只暴露“逻辑块寻址 + 按块/按字节读写”，不感知文件系统格式，也不转换 Linux errno；
  错误统一走 `DriverResult`（`DriverError` 分类来自 `driver-api`）。
- `BLOCK_SIZE` 固定为 512；`Lba` 从 0 起算。`read_bytes` / `read_prefix` 通过临时整段块缓冲
  实现任意字节对齐读取，设备侧仍只看到整倍数 `block_size` 的 `read_blocks` 调用。
- 注册顺序稳定：`register_block_device` 返回的下标即全局表中的位置；`first_block_device()`
  常用于根文件系统绑定单盘场景。
- DMA / HAL：virtio 队列与内部缓冲通过 `Hal::dma_alloc` 向全局帧分配器申请**物理连续、页
  对齐、已清零**的内存；恒等映射下 `paddr == vaddr`。
- 各 transport（MMIO/PCI）共用同一套帧分配策略：`virtio,mmio` 对应 RISC-V；PCI
  transitional/modern（`pci1af4,1001` / `pci1af4,1042`）对应 LoongArch。
- 块缓存为写穿（write-through）LRU：连续未命中区间合并为单次 `read_blocks`，读数据采用
  二次命中准入；`capacity_blocks` 为 0 时退化为直接透传底层设备。
- 缺失 virtio-blk 时 `init_after_boot` 会输出警告日志，根文件系统可能表现为未挂载。

## 调用链路

引导期注册（RISC-V 为例）：

```text
probe_virtio_devices()
  -> block_subsystem_claims_device(compatibles, DeviceType::Block)
  -> VirtioBlkDevice::from_mmio(mmio)
  -> BlockCacheManager::wrap(...)              // 启用 impl-block-cache 时
  -> register_block_device(SharedBlockDevice)  // 返回稳定下标
```

上层访问：

```text
文件系统 / 根文件系统
  -> first_block_device() / block_device_at(index)
  -> BlockDevice::read_bytes / read_blocks / write_blocks
  -> 底层 virtio-blk 按 LBA 访问（缓存层在中间拦截未命中）
```

## 各实现功能

### block-api / 块设备 API

主要实现在 `block-api/api-v0/src/lib.rs`：

- 提供逻辑块寻址：`Lba` 从 0 起算，可从 `usize` / `u64` 构造，`BLOCK_SIZE` 固定 512。
- 提供按块与按字节读写：`BlockDevice` 实现 `read_blocks` / `write_blocks`（`buf` 长度须为块
  大小的整数倍，不支持写返回 `Unsupported`），并默认用整段块缓冲实现任意字节对齐的
  `read_bytes` / `read_prefix`。
- 提供稳定注册表：`register_block_device` 返回稳定下标，`first_block_device` 供根文件系统
  绑定单盘，另有 `block_device_at` / `block_device_count`。

### impl-virtio-mmio / RISC-V VirtIO 块

- 从 DTB 枚举得到的 MMIO 窗口初始化 virtio-blk（`VirtioBlkDevice::from_mmio`）。
- 通过恒等映射帧分配向帧池申请连续物理页（`VirtioMmioHal`），不连续或 OOM 时整体回滚。

### impl-virtio-pci / LoongArch VirtIO 块

- 走 PCI ECAM 枚举并初始化 virtio-blk（`VirtioPciBlkDevice`），为 BAR 分配 MMIO 地址并开启
  `MEMORY_SPACE` / `BUS_MASTER`。

### impl-block-cache / 块缓存

主要实现在 `block-impl/impl-block-cache/src/lib.rs`：

- 用 `BlockCacheManager::wrap` 包装任意 `BlockDevice` 并提供写穿 LRU 缓存，对上仍实现同一
  trait。
- 合并连续未命中为单次底层 `read_blocks`，读数据二次命中准入，避免顺序扫描把一次性块复制
  进数据缓存；`capacity_blocks` 为 0 时退化为直接透传。

### impl-dummy / 占位实现

- 无硬件场景的占位块设备，配合 `impl-dummy` feature 使用。
