# T08：virtio-net NAPI

## 任务内容

把 virtio-net 的轮询改成 NAPI：中断触发后 ack + 关中断，按预算 poll 收包，耗尽后
重开中断；无包可收时 poller 阻塞在 waitqueue 而非定时 sleep。

## 实施方案

1. `NetworkDevice` 暴露 `ack_interrupt/enable_interrupts/disable_interrupts` 与 `receive` 非阻塞语义（已有 `can_recv`）。
2. 网络适配层（smoltcp adapter）与 poll 入口改为 NAPI 调度：
   - `os/components/wateros-network/network-impl/impl-smoltcp/src/adapter.rs`
   - `os/components/wateros-network/network-impl/impl-smoltcp/src/stack/poll.rs`
3. `os/src/main.rs` 的 `network_poller_task` 改为「IRQ/bottom-half 唤醒 + 预算 poll」，去掉 `sleep_for_ticks(1)` 忙轮询。
4. RISC-V MMIO 与 LoongArch PCI 都需对应 ack；先 RISC-V，LoongArch 随 T09 对齐。

## 涉及文件

- `os/components/wateros-driver/driver-network/network-api/api-v0/src/lib.rs`
- `os/components/wateros-driver/driver-network/network-impl/impl-virtio-mmio/src/lib.rs`
- `os/components/wateros-driver/driver-network/network-impl/impl-virtio-pci/src/lib.rs`
- `os/components/wateros-network/network-impl/impl-smoltcp/src/adapter.rs`
- `os/components/wateros-network/network-impl/impl-smoltcp/src/stack/poll.rs`
- `os/src/main.rs`

## CodeGraph 查询命令

```bash
codegraph explore "NetworkDevice can_recv receive ack_interrupt enable_interrupts"
codegraph explore "network_poller_task poll_at_millis poll_socket_events"
codegraph impact NetworkDevice
```

## 验收方式

- 静态：双架构 check。
- 功能：网络收发、socket 收包正确；对比轮询版 CPU 空转下降。
- 回归：`network_poller_task` 不再无事件空转。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
```

## 任务简报

完成后写 `history/08-net-napi.md`。
