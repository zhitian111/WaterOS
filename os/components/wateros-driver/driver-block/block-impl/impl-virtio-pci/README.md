# VirtIO-PCI Block 实现手册

[Block API](../../block-api/api-v0/README.md) · [LoongArch 机器探测](../../../driver-impl/impl-qemu-loongarch64-virt/README.md) · [MMIO 后端](../impl-virtio-mmio/README.md)

该 crate 为没有固件预分配 BAR 的裸机环境扫描并初始化 VirtIO block PCI function。它同时承担 PCI 配置写入、VirtIO capability transport 和块数据面，因而失败边界比 MMIO 版本更宽。

## 数据结构

- `VirtioPciProbeInfo`：成功设备的 bus/device/function、vendor/device ID，只供诊断，不拥有配置空间。
- `VirtioPciBarAllocator { next, end }`：在半开区间 `[next,end)` 单调分配 MMIO BAR；没有 free、快照或回滚。
- `VirtioPciBlkDevice`：持有 `VirtIOBlk<VirtioPciHal, PciTransport>`。

BAR 按 `next_power_of_two(size).max(16)` 对齐，然后检查地址加法和窗口上界。这里的 `next_power_of_two` 不是 checked 版本：畸形的超大 BAR size 可 panic，应改成 `checked_next_power_of_two()` 并映射为 `Unsupported`。

## 探测和初始化调用链

```text
probe_first_from_ecam / probe_first_from_mmio_cam
  -> probe_first_from_config(config_base, Cam, allocator)
  -> MmioCam::new + PciRoot::new
  -> enumerate_bus(0)
  -> pci::virtio_device_type == Block
  -> from_pci_root
       -> assign_memory_bars
       -> command |= MEMORY_SPACE | BUS_MASTER
       -> PciTransport::new（解析 vendor capability）
       -> VirtIOBlk::new（协商 feature、创建队列）
  -> 返回第一个设备及 ProbeInfo
```

`unsafe` 调用者必须保证 config base 非空、在内核地址空间可访问，并覆盖所选 CAM/ECAM 的完整配置窗口。当前只枚举 bus 0，不遍历 bridge 后的 secondary bus，也不处理热插拔；“返回 None”只表示 bus 0 未找到。

`assign_memory_bars` 检查每个 BAR：memory BAR 被重新分配，Below1MiB 拒绝，32 位 BAR 要求地址不超过 `u32::MAX`，64 位 BAR写两项；I/O BAR 只记录日志并保持禁用。平台给 block、net、GPU、input 的 allocator 区间必须互不重叠，也必须已映射为设备内存。

## 重要的非事务性

当前初始化不是事务：allocator 的 `next`、已经写入的 BAR、`MEMORY_SPACE` 和 `BUS_MASTER` 都不会在后续 capability/queue 初始化失败时复原。因而：

- 失败 function 不会进入 block registry，但会消耗 BAR 窗口；
- 可能留下 bus-master-enabled 的半初始化设备；
- 重试会从新的 `next` 再分地址；
- 不能把 `Err` 理解成“硬件状态未改变”。

正确修复方式是在写配置前保存 allocator cursor、原 BAR 和 command，任一步失败都按反序恢复；或先用只读 probe 计算完整计划，通过后一次提交。只有成功对象才能被平台注册。

## DMA 与数据面

`VirtioPciHal` 和 MMIO HAL 一样逐页取 frame，并假设栈式 allocator 在无并发插入时返回递减连续 PPN；随后清零并使用恒等映射。`share` 把 VA 当 PA，没有 cache maintenance、bounce buffer 或 IOMMU。PCI 开启 bus master 前，平台必须保证所有这些物理地址在设备 DMA mask 内。

`Vec` 元数据会使用内核 heap；清零长度 `pages * PAGE_SIZE` 未 checked。多页连续性不是 allocator 原子保证，SMP/碎片化都可能导致偶发初始化失败。详见 MMIO 后端的 DMA 修复方案。

块方法的边界与 MMIO 后端一致：API 范围检查 → LBA 转 `usize` → vendor 同步读写，错误映射为 `IoError`，flush 显式转发。

## 锁和生命周期

`PciRoot` 只在探测期间存在；设备中的 `PciTransport` 保存运行所需 capability 映射。配置空间、BAR 映射和 DMA 内存都必须覆盖设备寿命。注册层 mutex 串行所有 `&mut self` 操作，持锁期间禁止睡眠和反向进入 VFS。

`VirtioPciProbeInfo` 可复制，不能拿它判断设备仍存活。若实现 remove/hot-unplug，顺序应为摘除 registry、阻止新 I/O、排空队列、禁用 bus master、释放 DMA/BAR，不能直接 Drop 正在服务请求的对象。

## 扩展示例

支持多块盘时，不应继续使用 `probe_first_*`。新增 `probe_all_from_config`，先收集所有 `(DeviceFunction, IDs)`，再逐个事务初始化；单个 function 的失败策略需明确是跳过还是整批失败。设备节点顺序应由稳定 BDF 排序，而非 BAR 分配成功的偶然顺序。

## 回归清单

- ECAM 与 MmioCam、无设备、错误 ID、modern/transitional VirtIO；
- bus 0 以外设备明确不被当前实现发现；
- 32/64 位、Below1MiB、I/O、零 size、极大 size、窗口耗尽 BAR；
- 畸形/循环/越界 capability，初始化失败后检查 BAR、command 和 allocator 状态；
- DMA OOM、非连续帧、DMA mask、反复 probe 的 frame/heap 基线；
- 首尾 LBA、越界、非整块 buffer、读写 flush 数据一致性；
- 多 function 时第一个选择稳定，多个驱动 BAR 区间绝不重叠。
