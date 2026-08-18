# VirtIO-PCI Input 实现手册

[Input API](../../input-api/api-v0/README.md) · [LoongArch 探测](../../../driver-impl/impl-qemu-loongarch64-virt/README.md) · [MMIO Input](../impl-virtio-mmio/README.md)

该实现是四类 PCI VirtIO 后端中唯一默认返回 bus 0 上全部匹配 function 的驱动。每个键盘、鼠标或 tablet 都有独立 transport、metadata 和事件队列。

## 数据结构与调用链

- `VirtioInputPciProbeInfo`：成功 function 的 BDF 和 IDs；
- `VirtioInputPciBarAllocator`：`[next,end)` 内单调分配，使用 checked next-power-of-two；
- `VirtioInputPciDevice`：`VirtIOInput<PciTransport>` 加缓存的 `InputDeviceInfo`。

```text
probe_all_from_ecam
  -> enumerate_bus(0)，收集所有 VirtIO Input candidate
  -> 对每个 candidate 调 from_root
       -> assign_memory_bars
       -> enable MEMORY_SPACE | BUS_MASTER
       -> PciTransport::new -> VirtIOInput::new -> query_info
  -> 返回 Vec<(device, probe_info)>
  -> 平台逐个注册并建立 evdev 节点
```

先收集 candidate 是为了避免边枚举边可变配置 root，但不是事务。若后面的 function 初始化失败，`?` 会让整个函数返回 Err，先前构造在局部 `result` 中的设备被 Drop；然而所有已经推进的 BAR allocator、写入的 BAR 和 command 都不会回滚。源码也不是“坏 function 自动跳过、保留成功设备”的语义。比赛中修改失败策略前必须先决定整批失败还是逐个跳过。

## PCI 限制

只扫描 bus 0 和 ECAM，不遍历 bridge，也没有 legacy MmioCam 入口或 hotplug。memory BAR 被重新分配；Below1MiB 拒绝，I/O BAR保持禁用，32 位地址检查上限。input 的 BAR 窗口必须与 block/net/GPU 区间不重叠并已由页表映射。

配置和 allocator 没有 rollback。可靠实现应保存原 command/BAR/cursor，并在任一 transport、DMA 或 metadata 阶段失败时反序恢复；至少要在失败后撤销新增 BUS_MASTER。

## DMA、事件与元数据

HAL 逐页取 frame、验证递减连续、恒等映射。地址和清零长度使用 unchecked 乘法；空地址用 dangling 返回且已分 frames 不回收，dealloc 对零地址也无 guard。具体修复方案见 MMIO Input 手册。

元数据判别与 MMIO 版相同：name + EV_REL/EV_ABS，查询失败降级，不中止设备。`pop_event` 非阻塞并原样保留 type/code/value。上层 worker 必须在释放设备锁后再写 evdev 队列和唤醒等待者。

## 扩展示例：容错枚举

若目标是“坏鼠标不影响键盘”，返回类型应携带逐 BDF 结果，例如成功列表加 `(ProbeInfo, DriverError)` 诊断列表。每个 function 使用独立 BAR transaction；失败 rollback 后继续。注册索引按 BDF 排序，避免一次失败令 `/dev/input/eventN` 全部漂移。

## 回归清单

- 0/1/多个 input function，设备顺序与 BDF 稳定；
- 非 bus 0 设备明确不可见；错误/畸形单 function 的整批失败现状；
- BAR 各类型、窗口耗尽、capability 失败后的配置/allocator 状态；
- 多页 DMA 并发、OOM、零地址与失败 frame 泄漏；
- keyboard/mouse/tablet metadata 及每项查询失败；
- 多设备完整 SYN 事件序列、空队列和突发事件；
- worker 停止、registry 摘除、设备 Drop 的无 UAF 顺序。
