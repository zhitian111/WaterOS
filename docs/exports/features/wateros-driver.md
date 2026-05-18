# wateros-driver 功能快照

## 用途

本文件记录 **`wateros-driver`** 组件在当前仓库中的**功能边界与默认行为**，供与 `docs/architecture/snapshot.md`、根 `os/src/main.rs` 对照。更完整的说明见 **`docs/guides/device-driver.md`**。

## 事实来源

- `os/components/wateros-driver/Cargo.toml`
- `os/components/wateros-driver/src/lib.rs`
- `os/components/wateros-driver/driver-impl/impl-qemu-riscv64-opensbi/src/lib.rs`
- `os/src/main.rs`（`qemu-riscv64-opensbi`）

## 当前状态（快照）

- **聚合导出**：`api`、`block`、`character`、`network`；入口 **`init_when_boot`**、**`init_after_boot`**、**`test`**。
- **默认实现**：**`impl-qemu-riscv64-opensbi`**（feature 默认开启）。
- **`active_impl`**：与默认 impl 同名 re-export，供内核主线 **`driver::active_impl::init_after_boot()`** 等形式调用（与聚合层 `init_after_boot` 包装并存，注意二者日志与错误处理差异）。
- **DTB**：启动早期 **`init_when_boot`** 记录 DTB 物理基址；**`init_after_boot`** 内用 **`fdt`** 遍历，**`reg`** 通过 **`FdtNode::reg()`** 按父总线 **`#address-cells` / `#size-cells`** 解析。
- **virtio-mmio**：读取 magic 与 **`device_id`**；**`device_id == 2`** 时识别为块设备并经由 **`wateros-driver-block`** 的 **`impl-virtio-mmio`** 子 crate 初始化 **`VirtioBlkDevice`**；空槽位常见 **`device_id == 0`**，类型为 **`Unknown`**。
- **块设备注册**：成功后进入 **`driver-block`** 全局表。根 crate 在 **`qemu-riscv64-opensbi`** 下启用 **`driver/impl-block-cache`** 时，QEMU 实现以 **`CachingBlockDevice`**（写穿 LRU，默认 64 块）包装 **`VirtioBlkDevice`** 再注册；对上仍为 **`BlockDevice`**，文件系统无需改动。
- **devfs**：实现内在 **`init_after_boot`** 末尾通过 **`wateros-fs`** 聚合 crate 的 **`devfs::active_impl::refresh()`** 同步设备节点路径（如 **`/dev/vblk0`**），不直接依赖 `fs-devfs` 子 crate。

## 缺口与后续关注点

- **`IrqLine` / 中断**：当前多为 DTB 信息记录，**未**与平台中断控制器完成端到端接线。
- **网络**：可识别 virtio-net 类 **`device_id`**，**无**完整驱动与协议栈挂载路径。
- **字符设备子系统**：仍以 **dummy** 为主。
- **文档与注释**：公共 API 的 `///` 与跨组件契约可随接口稳定继续补齐。

## 同步要求

当默认 feature、启动接线、`active_impl` 导出或 DTB/virtio/devfs 协作方式变化时，应同步更新：

- 本文件；
- **`docs/guides/device-driver.md`**；
- 必要时 **`docs/exports/public-api/wateros-driver.md`** 与 **`docs/exports/impl-guide/wateros-driver.md`**。
