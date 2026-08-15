# T06-e 任务简报：block 读去 FS 持锁 + virtio 完成健壮性

- 完成日期：2026-08-15
- commit：`7ff1185b`（`[feat] 块 IRQ 改 Linux 式单请求等待并修复 virtio 通知抑制（默认关闭）`）

## 结论

block IRQ 链路已从「IRQ 触发/死锁」推进到「IRQ 完成路径正常工作」，剩余问题在
页面缓存/VFS 与 IRQ 异步读的交互，而非 block 驱动本身。

## 实际改动

- `impl-virtio-mmio`：等待改为 Linux 式**单请求在途**——原子 `in_flight` 门闩 +
  `wait_current_while` 睡眠，等待期间不持设备自旋锁；IRQ top-half 只 ack，
  bottom-half 在可调度上下文 `notify_irq` 唤醒。
- `vendor/virtio-drivers/src/device/blk.rs`：`read_blocks_nb/write_blocks_nb`
  改为无条件 `transport.notify`（强制 kick），修复 event_idx 通知抑制导致设备
  不处理新请求的根因。
- `register.rs`：IRQ 开关默认 `false`；top-half ack + bh notify 接线。
- `main.rs`：全局中断就绪后 `enable_runtime_dispatch`。

## 验证

- `make rv_check` / `make la_check` 通过。
- IRQ 模式：top ack → bh notify → waiter wake 链路完整；`avail/used/last_used`
  同步推进；读回 `buf` 头部字节为真实内容（ext4 元数据、RISC-V 机器码、文本）。
- 同步路径（开关关闭）cagent 全绿（rebase 后回归）。

## 剩余问题与下一步

IRQ 模式开启时 cagent 卡在 `paged_handle seek` 循环（bash 反复 seek/重读脚本），
但 block 数据正确。下一步专项审计 `impl-page-cache` 的 `install_page` 锁外读 +
回查装填在 IRQ 异步完成下的空/旧页竞态，或做「同步 vs IRQ 同页字节」对比。
