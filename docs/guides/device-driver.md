# 设备驱动模块说明（当前快照）

## 用途与范围

本文说明内核一级组件 **`wateros-driver`** 在当前仓库中的职责划分、与 **DTB / OpenSBI 引导参数** 的接线方式、**QEMU virt + virtio-mmio** 路径下的实际行为，以及与 **`wateros-fs`（含 devfs）** 的协作边界。

**不在本文范围**：用户态驱动模型、`wateros-vfs` 完整挂载语义、除 virtio-blk 以外的具体硬件驱动实现细节。

## 事实来源

以下路径与本文描述保持一致，修改实现时应同步核对并视情况更新本文及相关 `exports/` 快照。

- `os/components/wateros-driver/Cargo.toml`
- `os/components/wateros-driver/src/lib.rs`
- `os/components/wateros-driver/driver-impl/impl-qemu-riscv64-opensbi/`
- `os/components/wateros-driver/driver-api/api-v0/`
- `os/components/wateros-driver/driver-block/`
- `os/src/main.rs`（`qemu-riscv64-opensbi` 下的启动顺序）
- `os/components/wateros-fs/src/lib.rs`（根文件系统初始化与 devfs 刷新）

## 组件结构（API / impl 范式）

`wateros-driver` 遵循「聚合 crate + 子系统 + 平台 impl」的组织方式：

| 层级 | 路径（相对 `os/components/wateros-driver/`） | 职责 |
|------|-----------------------------------------------|------|
| 聚合层 | `src/lib.rs` | 导出 `api`、`block`、`character`、`network`；提供 `init_when_boot`、`init_after_boot`、`test`；通过 feature 选择 `active_impl`。 |
| 设备抽象 API | `driver-api/api-v0/` | `DeviceInfo`、`DeviceType`、`MmioRegion`、`DriverError` 等与具体总线无关的公共类型。 |
| 块设备子系统 | `driver-block/` | `BlockDevice` trait、`register_block_device`、全局块设备表（`SharedBlockDevice`）；**VirtIO-MMIO 块实现**位于 `driver-block/block-impl/impl-virtio-mmio/`，由 feature **`impl-virtio-mmio`** 启用并由 `wateros-driver` 的 **`impl-qemu-riscv64-opensbi`** 自动打开。可选 **写穿 LRU 块缓存** `driver-block/block-impl/impl-block-cache/`（**`CachingBlockDevice`**），由 **`wateros-driver`** 的 **`impl-block-cache`** 与根 crate **`qemu-riscv64-opensbi`** 下的 **`driver/impl-block-cache`** 打开；注册 virtio-blk 时用其包装底层设备，对上接口不变。 |
| 字符 / 网络子系统 | `driver-character/`、`driver-network/` | 当前以 **dummy impl** 为主，占位与自检。 |
| 平台实现 | `driver-impl/impl-qemu-riscv64-opensbi/` | QEMU RISC-V + OpenSBI：DTB 扫描、virtio-mmio 探测、调用 **`driver-block`** 中的 **`VirtioBlkDevice`** 完成实例化；若启用 **`block-cache`** feature，则以 **`CachingBlockDevice`** 包装后再 **`register_block_device`**；否则直接注册裸设备。末尾触发 devfs 同步。 |
| 空实现 | `driver-impl/impl-dummy/` | 无硬件时的编译与链接占位。 |

**默认 feature**（见组件根 `Cargo.toml`）：`impl-qemu-riscv64-opensbi`，即默认启用 QEMU OpenSBI 路径下的真实探测逻辑。

## 启动与初始化顺序

在 `os/src/main.rs` 的 `qemu-riscv64-opensbi` 路径中，与设备相关的主线顺序为：

1. **`driver::init_when_boot(boot_arg1)`**  
   仅保存引导传入的 **DTB 物理基址**（具体语义由 `platform::boot::BootArgs` 约定，与 OpenSBI 传递的 `a1` 一致）。

2. 控制台、日志、堆、**`platform::arch::init()`** 以及 MM 自检（含可选分页冒烟测试，取决于根 crate 的 `impl-sv39` 等 feature）。

3. **`driver::active_impl::init_after_boot()`**  
   当前内核主线直接调用 **`active_impl`**，以便获得 **`DriverResult`**；聚合层的 `driver::init_after_boot()` 为另一套包装（内部用 `log` 打失败），二者不要混用以免排查困惑。

4. 若上一步为 **`Ok`**，再执行 **`fs::init()`**、**`fs::test()`**（根文件系统与 **`impl-ext4`** 自检依赖 devfs 上已可见的块设备路径）。

```mermaid
flowchart LR
    boot[BootArgs_a1_DTB]
    saveDtb[driver_init_when_boot]
    mmSelfTest[mm_self_test_optional_paging]
    drvScan[active_impl_init_after_boot]
    devfsSync[devfs_refresh_via_fs]
    fsInit[fs_init_rootfs]

    boot --> saveDtb
    saveDtb --> mmSelfTest
    mmSelfTest --> drvScan
    drvScan --> devfsSync
    devfsSync --> fsInit
```

## QEMU OpenSBI 实现：DTB 与 virtio-mmio

实现位置：DTB 与枚举逻辑在 **`driver-impl/impl-qemu-riscv64-opensbi/src/lib.rs`**；VirtIO 块设备类型在 **`driver-block/block-impl/impl-virtio-mmio/src/lib.rs`**。

### DTB 解析

- 使用 **`fdt`** crate（`Fdt::from_ptr`）在启动阶段记录的 DTB 物理地址上解析整棵树。
- 具备 **`compatible`** 的节点会进入 **`DeviceInfo`** 列表（用于枚举与日志）；**MMIO 首段** 使用 **`FdtNode::reg()`** 解析，其内部按 **父节点** 的 **`#address-cells` / `#size-cells`** 解释 `reg`，与 QEMU virt 上 `soc` 下常见的 **2+2 cells** 一致。
- **`virtio,mmio`** 节点：在解析出有效 `reg` 后，对 MMIO 基址做 **virtio 规范 magic（`0x74726976`）与 `device_id`** 探测，映射到 **`DeviceType::Block` / `Network` / `Unknown`**。

### 关于 `device_id == 0`

QEMU 常在 DTB 中描述 **多个 virtio-mmio 槽位**；未接入设备的槽位在读 **`device_id`** 时通常为 **0**，类型为 **`Unknown`**。这是 **预期现象**，不是解析失败。

### virtio-blk 注册与 devfs

- 仅当探测结果为 **`DeviceType::Block`** 时，尝试 **`VirtIOBlk`** 初始化，成功后以 **`CachingBlockDevice`** 包装（若编译启用了 **`block-cache`** feature）或直接使用裸 **`VirtioBlkDevice`**，再调用 **`register_block_device`** 写入 **`driver-block`** 的全局表。
- **`init_after_boot`** 末尾通过依赖 **`wateros-fs`** 聚合 crate 暴露的 **`devfs::active_impl::refresh()`**，根据 **`block_device_count()`** 生成 **`/dev/vblk{N}`** 等节点；**不要**从驱动 impl 直接依赖 `fs-devfs` 子目录 crate，以保持层级一致。

### 诊断日志（当前行为）

实现中会输出 **`[driver] dtb virtio-mmio: ...`** 等信息，便于区分「空槽位」与「真实 virtio-blk」。当 **`block_device_count == 0`** 时还会输出与 **QEMU 是否挂载 `virtio-blk-device`**、**MMU 是否映射 MMIO** 相关的提示性 **WARN**。

## 与文件系统（devfs）的关系

- **devfs 不实现块存储**：它只维护 **设备路径 → `SharedBlockDevice`** 的绑定，数据读写仍走 **`driver-block`** 注册的实现。
- **根文件系统挂载**（`wateros-fs` 的 `init`）在驱动成功之后执行，典型路径为 **`/dev/vblk0`**（由 devfs 默认选择逻辑决定，见 `fs-rootfs` 与 **`fs-impl/impl-ext4`** 相关代码）。

若 **`block_devices_registered == 0`**，则 **`devfs` 节点数为 0**、`fs::init` 可能报 **`NotMounted`**——此时应优先排查 **驱动探测与 QEMU 设备模型**，而不是假设 devfs 实现损坏。

## 当前能力边界与缺口

以下条目反映**当前快照**，后续演进应在变更实现时更新本文与 `docs/exports/features/wateros-driver.md`。

| 能力 | 状态 |
|------|------|
| DTB 枚举与 `DeviceInfo` | 已有；以 `compatible` + `reg` + virtio header 为主。 |
| virtio-blk（MMIO） | 已有；与 `virtio-drivers` 对接。 |
| devfs 节点刷新（经 `wateros-fs`） | 已有。 |
| 中断实际挂接与 `IrqLine` 使用 | **未接线**；`DeviceInfo.irq` 多为信息记录。 |
| virtio-net 等网络设备 | **类型可识别**，无完整网络栈注册路径。 |
| character / network 子系统 | **以 dummy 为主**，占位与自检。 |
| 非 QEMU / 非 OpenSBI 平台 | 依赖切换 **`impl-dummy`** 或其它未来 impl。 |

## 相关导出文档

- 功能快照：`docs/exports/features/wateros-driver.md`
- 公共 API 摘要：`docs/exports/public-api/wateros-driver.md`
- 新增 impl 检查清单：`docs/exports/impl-guide/wateros-driver.md`

## 维护约定

当发生以下任一情况时，应更新本文及 `exports/features` 中对应条目：

- 默认 feature 或 `active_impl` 切换；
- DTB 解析策略、virtio 探测条件、或 devfs 同步调用路径变化；
- `os/src/main.rs` 中驱动与 `fs` 的启动顺序调整。
