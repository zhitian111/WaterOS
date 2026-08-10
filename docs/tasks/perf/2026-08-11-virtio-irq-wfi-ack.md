# VirtIO IRQ/WFI + 设备 ack 方案（2026-08-11）

## 为什么选择这里

上一版 `perf/virtio-irq-wfi` 在第一次块读取 `lba=2` 后停在 WFI，PLIC claim 从未
进入。可能原因是 VirtIO MMIO 设备中断状态没有在 trap 中 ack，导致设备认为中断
仍 pending，或者完成通知没有进入中断路径。

这次在最小 WFI 等待上补上真正的设备侧中断 ack：

```text
PLIC external trap
  -> claim IRQ
  -> VirtioBlkDevice::ack_interrupt()
  -> PLIC complete
```

## 方案

1. 恢复 RISC-V PLIC enable/claim/complete 和 `sie.SEIE`。
2. 增加 `plic::register_handler`：保存 `irq -> fn(irq)`，外部中断 trap 中调用。
3. `VirtioBlkDevice` 暴露 `ack_interrupt()`。
4. `IrqBlockDevice` 注册 block IRQ handler，handler 只 ack VirtIO interrupt，不碰
   设备锁。
5. 同步 read/write 使用 `read_blocks_nb`/`write_blocks_nb`，等待循环临时关闭
   timer/soft 中断并执行 WFI；外部中断唤醒后再 `complete_*`。

## 为什么这么做

1. 先证明 VirtIO 中断链路本身可用，而不是直接引入完整异步请求生命周期。
2. 如果仍停在 WFI，可以确认问题在设备通知或 PLIC 路由，而不是缺 ack。

## 验收

- 双架构 Final check 通过。
- 180 秒 smoke 能通过 rootfs 挂载并进入 BuildStorm。
- 若仍停滞，回退并记录中断链路证据。

## 实测结果（2026-08-11）

双架构 Final check 通过，但 180 秒 smoke 仍在第一次 FS 初始化后停滞，与上一版
IRQ/WFI 现象一致。即使 PLIC handler 已调用 VirtIO `ack_interrupt()`，设备完成
中断仍未唤醒等待 CPU，说明问题不是缺少设备 ack，而是 VirtIO used-buffer 通知或
PLIC 路由链路本身需要完整设备 IRQ 生命周期。

实现已全部回退，仅保留本记录。
