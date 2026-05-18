# wateros-driver 公共 API 快照

## 用途

描述 **`wateros-driver`** 在默认 **`impl-qemu-riscv64-opensbi`** 下对 **`api`**、**`block`**、**`character`**、**`network`** 的整包再导出，以及根级 **bring-up / 自检** 入口与 **`active_impl`** 别名。

## 事实来源

- [`os/components/wateros-driver/Cargo.toml`](../../os/components/wateros-driver/Cargo.toml)
- [`os/components/wateros-driver/src/lib.rs`](../../os/components/wateros-driver/src/lib.rs)

## Feature（默认）

| Feature | 说明 |
|---------|------|
| **`default`** | `api-v0` + **`impl-qemu-riscv64-opensbi`** → 打开 **`block/impl-virtio-mmio`**；根 **`qemu-riscv64-opensbi`** 另启用 **`driver/impl-block-cache`** → **`block/impl-block-cache`**（virtio-blk 注册前 **`CachingBlockDevice`** 包装）。 |
| **`impl-dummy`** | 与 **`impl-qemu-riscv64-opensbi`** 二选一用于 **`active_impl`**；同时启用会导致重复别名 **编译失败**。 |
| **`impl-block-cache`** | 打开 **`block/impl-block-cache`** 与 **`impl-qemu-riscv64-opensbi/block-cache`**；单独关闭时勿从根 feature 链拉取 **`driver/impl-block-cache`**。 |

## 聚合层导出

| 项 | 说明 |
|----|------|
| **`pub mod api`** | **`wateros-driver-api-v0`** 根公共项（**`DeviceType`**、**`MmioRegion`**、**`IrqLine`**、**`SupportedDeviceEntry`**、**`DeviceInfo`**、**`DriverError`** / **`DriverResult`**、**`test`** 等）。 |
| **`pub mod block`** | **`wateros-driver-block`**：块 API **`api_v0::*`**、**`BLOCK_SUPPORTED_DEVICES`**、**`supported_devices`**、**`register_block_device`**、**`first_block_device`** 等；**`#[cfg(feature = "impl-virtio-mmio")]`** 下 **`VirtioBlkDevice`**；**`#[cfg(feature = "impl-block-cache")]`** 下 **`CachingBlockDevice`**、**`BlockCacheConfig`**。 |
| **`pub mod character` / `pub mod network`** | 各自子 crate 根再导出（当前以 **`add`**、**`supported_devices`** 等为主，见子 **`lib.rs`**）。 |
| **`active_impl`** | **`impl_qemu_riscv64_opensbi`** 或 **`impl_dummy`**（由 feature 二选一）。 |
| **`supported_device_entries()`** | 合并 block/character/network 的 **`supported_devices()`** 静态表。 |
| **`init_when_boot(dtb_pa)`** | QEMU 实现保存/解析 DTB 起点；dummy 忽略参数。 |
| **`physical_ram_end_exclusive()`** | QEMU 路径从实现读取；否则回退 **`wateros_base_config::mm::QEMU_VIRT_PHYS_RAM_END`**。 |
| **`init_after_boot()`** | DTB 扫描、virtio 注册等；错误仅打日志。 |
| **`test()`** | 依次 **`api_v0::test`**、**`block::test`** 及（QEMU 下）**`impl_qemu_riscv64_opensbi::test`**。 |

## 维护要求

默认 impl、块 virtio 路径或根导出变化时，同步更新本文件、**`docs/exports/features/wateros-driver.md`** 与 **`docs/guides/device-driver.md`**（若叙述交叉）。
