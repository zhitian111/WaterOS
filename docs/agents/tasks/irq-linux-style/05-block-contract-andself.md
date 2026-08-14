# T05：BlockDevice 契约改 `&self` + 同步回退

## 任务内容

把 `BlockDevice` 从 `&mut self` 改为 `&self` + 实现内自持锁，并把
`SharedBlockDevice` 从 `Arc<Mutex<Box<dyn BlockDevice>>>` 改为 `Arc<dyn BlockDevice>`。
本任务只重构锁模型，I/O 仍同步完成，行为与 main 等价，为 T06 异步化铺路。

## 实施方案

1. 改 `block-api` 的 `BlockDevice` trait 签名为 `&self`，并明确 `Send + Sync` 边界。
2. 各实现内部引入短锁：
   - `impl-virtio-mmio`、`impl-virtio-pci`：设备级锁；
   - `impl-block-cache`：缓存元数据/数据锁与 backend 分离；
   - sample/测试实现同步。
3. `register_block_device` 等注册表与 `fs-impl/*` 调用点同步；`another_ext4` 适配层只改类型
   引用，不改变 I/O 完成方式。
4. LoongArch PCI 与 RISC-V MMIO 路径一起改，保持双架构 check。

## 涉及文件

- `os/components/wateros-driver/driver-block/block-api/api-v0/src/lib.rs`
- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/src/lib.rs`
- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-pci/src/lib.rs`
- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/*.rs`
- `os/components/wateros-fs/fs-impl/impl-{another-ext4,ext4,ext4-rs,ramfs}/**`（类型引用）
- `os/components/wateros-driver/driver-impl/impl-qemu-{riscv64-virt,loongarch64-virt}/src/register.rs`

## CodeGraph 查询命令

```bash
codegraph explore "trait BlockDevice SharedBlockDevice register_block_device"
codegraph impact BlockDevice
codegraph callers read_blocks
codegraph callers write_blocks
```

## 验收方式

- 静态：双架构 check。
- 单测：`impl-block-cache` 与 `block-api` host test。
- QEMU 功能：双架构 rootfs 挂载、读写、close/fsync 后一致性不回归。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
make rv_qemu_run_snapshot
make la_qemu_run_snapshot
```

## 任务简报

完成后写 `history/05-block-contract-andself.md`。
