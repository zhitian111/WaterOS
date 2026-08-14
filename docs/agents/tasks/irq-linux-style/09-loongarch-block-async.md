# T09：LoongArch 块异步对等

## 任务内容

把 T06 的异步 request_queue 完成路径对等到 LoongArch virtio-PCI 块设备，配合 T03
的 EIOINTC/PCI INTx/MSI-X 完成唤醒。T06 若已抽象好完成路径，本任务主要是 PCI 侧
ack 与中断路由接线。

## 实施方案

1. `impl-virtio-pci` 块驱动接入 `read_blocks_nb/write_blocks_nb/complete_*` 与
   `ack_interrupt/enable_interrupts`。
2. 注册 PCI 设备中断到 T03 的 irq domain；bottom-half 复用 T04。
3. 若 MSI-X 可行，评估其与 INTx 的取舍；优先保证功能正确，再谈性能。

## 涉及文件

- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-pci/src/lib.rs`
- `os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/register.rs`
- `os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/eiointc.rs`
- `os/components/wateros-irq/src/bottom_half.rs`

## CodeGraph 查询命令

```bash
codegraph explore "VirtioPciBlkDevice probe_first_from_ecam PciTransport ack_interrupt"
codegraph explore "probe_virtio_blk_pci register_devices"
codegraph impact VirtioPciBlkDevice
```

## 验收方式

- 静态：`make la_check && make rv_check`。
- QEMU 功能：LoongArch 块 I/O + iozone 最小集，无卡死/竞态。
- 功能全绿后进入 T12 性能对比。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make la_check && make rv_check
make kernel-la-final
make la_qemu_run_snapshot
```

## 任务简报

完成后写 `history/09-loongarch-block-async.md`。
