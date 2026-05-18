# wateros-driver 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时至少要更新 `wateros-driver/Cargo.toml`、对应 impl crate 的 `Cargo.toml`、聚合 `src/lib.rs` 以及需要接入的子系统组件。若是平台驱动实现，需同步检查平台和 DTB 初始化链。

**当前主线行为与 DTB/devfs 关系**（便于 impl 对齐）：见 **`docs/guides/device-driver.md`**。

**VirtIO 块设备实现位置**：`driver-block/block-impl/impl-virtio-mmio/`（crate `wateros-driver-block-impl-virtio-mmio`），由 **`wateros-driver-block`** 的 feature **`impl-virtio-mmio`** 接入；平台 impl **`impl-qemu-riscv64-opensbi`** 通过依赖 `driver-block` 并启用该 feature 完成注册。

**可选块缓存**：`driver-block/block-impl/impl-block-cache/`（`CachingBlockDevice`），由 **`wateros-driver-block`** 的 **`impl-block-cache`** 与聚合层 **`impl-block-cache`**、平台 **`impl-qemu-riscv64-opensbi`** 的 **`block-cache`** 联动；根 **`qemu-riscv64-opensbi`** 已打开 **`driver/impl-block-cache`**，使主线 QEMU 路径默认带写穿缓存。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 若需向 devfs 或根文件系统暴露设备：优先通过 **`wateros-fs`** 聚合层导出能力，避免驱动 impl 直接依赖 `fs-devfs` 等子 crate
- 相关导出文档是否已同步更新（**`docs/guides/device-driver.md`**、`docs/exports/features/wateros-driver.md` 等）
