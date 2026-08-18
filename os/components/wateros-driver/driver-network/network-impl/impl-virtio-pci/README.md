# VirtIO-PCI Network 实现手册

[Network API](../../network-api/api-v0/README.md) · [LoongArch 机器探测](../../../driver-impl/impl-qemu-loongarch64-virt/README.md) · [MMIO Network](../impl-virtio-mmio/README.md)

该实现扫描 bus 0 的第一个 VirtIO network PCI function，为其配置 BAR/bus master，创建 32 项队列和 2048 字节 RX buffers，再交给协议栈。

## 数据结构

- `VirtioNetPciProbeInfo`：成功 BDF 及 vendor/device ID；
- `VirtioNetPciBarAllocator { next, end }`：网络专用 `[next,end)` MMIO 窗口，无 free/rollback；
- `VirtioPciNetDevice`：`VirtIONet<VirtioPciNetHal, PciTransport, 32>`。

allocator 使用未 checked 的 `size.next_power_of_two()`；畸形极大 BAR 可 panic。需改为 `checked_next_power_of_two()`，并保留 16 字节最小对齐、checked add 和窗口检查。

## 探测调用链

```text
probe_first_from_ecam / probe_first_from_mmio_cam
  -> probe_first_from_config(config_base, Cam, allocator)
  -> enumerate_bus(0)，匹配 VirtIO Network
  -> from_pci_root
       -> assign_memory_bars
       -> command |= MEMORY_SPACE | BUS_MASTER
       -> PciTransport::new
       -> VirtIONet::new(..., RX_BUF_LEN=2048)
  -> 返回第一个 device + probe info
```

unsafe 调用者保证配置空间和 BAR 映射有效。当前不扫描 bridge/其它 bus，不支持 hotplug；I/O BAR 保持禁用，Below1MiB memory BAR拒绝，32/64 位 memory BAR重新写入。

## 非事务初始化

BAR cursor、已写 BAR 和 command 在 capability、DMA 或队列初始化失败时都不回滚。错误后可能留下 enabled bus master，并消耗网络 BAR 窗口；重试不是幂等操作。修复应在配置前保存 allocator cursor、原 BAR/command并反序恢复，或先完成只读规划再提交。只有完整成功的对象可进入 registry。

DMA HAL 逐页拿 frame并假定递减连续 PPN，使用恒等映射和 VA=PA share。SMP 并发/碎片会让多页请求偶发失败；`Vec` 使用 heap，清零长度未 checked。开启 BUS_MASTER 前还须确认设备 DMA mask 可达这些物理页。

## 数据面现状与已知契约违例

PCI 版本的 send/receive、固定 MTU、link 判断与 MMIO 版本相同：

- 小 RX buffer 时已经消费并 recycle 帧，再返回 `InvalidParam`，因此无法按 API 重试；
- 成功返回混用 `packet_len()` 和实际复制长度；
- recycle 失败只 warning；
- `can_recv || can_send` 表示队列 readiness，不是真实 link status。

修复必须同时应用两个 transport 后端，最好把数据面抽为共享适配层，避免一种平台仍丢包。协议栈持设备 mutex 时，驱动不能反向获取 socket/网络全局锁。

## 生命周期与扩展

配置空间、BAR、DMA buffers 和 PciTransport capability 都必须活到 registry 最后一个 `Arc`/poll 引用释放。热拔插顺序：从网络 registry 标记下线、阻止新 poll、排空/取消 TX/RX（含 pending RX）、禁用 bus master、释放 DMA，再回收 BAR。

多网卡不能继续依赖 `probe_first`。新增 all-probe 时按稳定 BDF 枚举，每个 function 使用独立事务；接口名/MAC 对应关系必须稳定，单卡失败策略要可诊断。

## 回归清单

- ECAM/MmioCam、modern/transitional ID、空 bus、非 bus 0 设备；
- 32/64/Below1MiB/I/O/极大 BAR、窗口不足与失败回滚；
- capability/queue/DMA 各阶段失败后的 command、cursor、frame/heap 基线；
- MAC、MTU feature、真实 link status 与 queue readiness；
- 小 buffer 重试同一帧、最大帧、连续帧顺序、recycle 失败恢复；
- ARP/ICMP/UDP/TCP、长时间吞吐及 SMP poll，无丢锁/死锁；
- 多网卡按 BDF 稳定注册且各驱动 BAR 窗口不重叠。
