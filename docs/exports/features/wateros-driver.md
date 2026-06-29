# wateros-driver — 已实现功能快照

## 用途

记录 `wateros-driver` 一级组件当前已落地能力、feature 组合与已知缺口。

事实来源：`os/components/wateros-driver/**` 源码与各 `Cargo.toml`；根 `wateros` 通过 `driver/impl-qemu-riscv64-opensbi` 或 `driver/impl-qemu-loongarch64-virt` 选择平台路径。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-driver`（聚合） | 导出 `api`/`block`/`character`/`network`；`init_when_boot`/`init_after_boot`/`test`；`active_impl` | 已实现 |
| `wateros-driver-api-v0` | `DeviceInfo`、`DeviceType`、`MmioRegion`、`DriverError` 等跨子系统模型 | 已实现 |
| `wateros-driver-block` | `BlockDevice` trait、全局注册表、DTB 绑定声明 | 已实现 |
| `wateros-driver-block-api-v0` | 块 API 契约与样例自检 | 已实现 |
| `block-impl-virtio-mmio` | VirtIO-MMIO 块设备（RISC-V QEMU 路径） | 已实现 |
| `block-impl-virtio-pci` | VirtIO-PCI 块设备（LoongArch QEMU 路径） | 已实现 |
| `block-impl-block-cache` | 写穿 LRU `CachingBlockDevice` 装饰器 | 已实现 |
| `block-impl-dummy` | 占位 | 已实现（占位） |
| `wateros-driver-character` | 字符设备注册表、UART 兼容声明、RTC/null stub | 已实现 |
| `wateros-driver-network` | 网卡注册表、smoltcp 协议栈、`socket_handles` | 已实现 |
| `network-impl-virtio-mmio` / `impl-virtio-pci` | VirtIO 网卡 | 已实现 |
| `network-impl-smoltcp` | `SmoltcpAdapter` + `stack` 模块 | 已实现 |
| `impl-qemu-riscv64-opensbi` | DTB 扫描、virtio-mmio blk/net、UART、devfs 同步 | 已实现 |
| `impl-qemu-loongarch64-virt` | PCIe ECAM 枚举 virtio-blk/net、UART、devfs | 已实现 |
| `impl-dummy` | 无硬件占位 | 已实现（占位） |

## Feature 矩阵（聚合层）

| Feature | 效果 |
|---------|------|
| `api-v0` | 启用各子系统 API v0 与平台 impl 的 `api-v0` 传递 |
| `default` | `["api-v0"]`（无平台 impl；须由根 crate 再开） |
| `impl-qemu-riscv64-opensbi` | `block/impl-virtio-mmio` + `network/impl-virtio-mmio` |
| `impl-qemu-loongarch64-virt` | `block/impl-virtio-pci` + `network/impl-virtio-pci` |
| `impl-block-cache` | 注册 virtio-blk 时用 `CachingBlockDevice` 包装 |
| `impl-dummy` | 平台占位（空） |

根 `wateros` 默认 `qemu-riscv64-opensbi` 同时打开 `driver/impl-qemu-riscv64-opensbi` 与 `driver/impl-block-cache`。

## 已实现能力

### 块设备

- **VirtIO-MMIO**（RISC-V）：DTB `virtio,mmio` + magic/`device_id` 探测 → `VirtioBlkDevice` → 可选块缓存 → `register_block_device`。
- **VirtIO-PCI**（LoongArch）：ECAM bus 0 扫描 → `VirtioPciBlkDevice` → 同上注册路径。
- **写穿块缓存**：未命中合并读、LRU 淘汰；`capacity_blocks` 来自 `base-config::BLOCK_CACHE_CAPACITY_BLOCKS`。
- **DMA**：`virtio-drivers` HAL 使用帧分配器，恒等映射 `paddr == vaddr`。

### 字符设备

- DTB NS16550 节点注册为 `SerialPortCharacterDevice`；无节点时 RISC-V 回退 `0x1000_0000` UART0。
- LoongArch 全局 `QemuLoongArch64Uart16550`（`0x1FE0_01E0`）。
- 内置 **RTC stub**（`hwclock` ioctl 在 syscall 层处理）与 **null stub**（`/dev/null` 语义）。

### 网络

- **VirtIO-MMIO / PCI** 网卡注册到全局表。
- **smoltcp**：`driver::network::stack::init` 配置 IP/路由；`poll`/`poll_at_millis` 驱动；socket 工厂与 `setsockopt` 子集（iperf/netperf 依赖）。
- **loopback**：无网卡时 `SmoltcpAdapter::loopback_only`；UDP/TCP 127.0.0.1 软件队列。
- **VFS 桥**：`socket_handles` 实现 `VfsIoHandle`（`impl-smoltcp` feature）。

### 平台 bring-up

| 路径 | 探测方式 | RAM 上界 |
|------|----------|----------|
| RISC-V OpenSBI | 全 DTB 扫描 + virtio header | DTB `memory@*` 或 `QEMU_VIRT_PHYS_RAM_END` |
| LoongArch virt | PCIe ECAM + DTB PCI 基址 | DTB 或 `0xb000_0000`（1G 机型的内核高段） |

`init_after_boot` 幂等；重复调用记录 WARN 并忽略。成功后经 `wateros-fs` 的 `devfs::refresh` 暴露 `/dev/vblk*` 等节点。

## 缺口与后续

- **中断**：`DeviceInfo.irq` 已解析，驱动未挂接 PLIC/APLIC 处理。
- **多盘 / 多网卡**：全局表支持多实例，devfs 与 syscall 策略仍以「首设备」为主。
- **块设备写**：virtio-blk 写路径依赖具体实现；缓存为写穿。
- **非 QEMU 平台**：仅 `impl-dummy` 占位，无 DTB/PCI 探测。
- **`driver::test()`**：聚合自检存在，根 `kernel_main` 默认不调用。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（块缓存、双平台 virtio、smoltcp 栈） |
