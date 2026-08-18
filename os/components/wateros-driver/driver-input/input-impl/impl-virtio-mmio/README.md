# VirtIO-MMIO Input 实现手册

[Input API](../../input-api/api-v0/README.md) · [RISC-V 探测](../../../driver-impl/impl-qemu-riscv64-virt/README.md)

本 crate 把一个 VirtIO-MMIO input function 变成非阻塞 `InputDevice`。平台负责 DTB 枚举和注册，上层 input/evdev 负责排队、等待和设备节点。

## 对象和构造链

`VirtioInputMmioDevice` 包含 `VirtIOInput` 与构造时缓存的 `InputDeviceInfo`：

```text
MmioRegion -> from_mmio -> MmioTransport::new -> VirtIOInput::new
           -> query_info -> 平台注册 -> input worker 周期 pop_event
```

空 base 返回 `InvalidParam`，transport 或 device 初始化失败映射为 `Unsupported`。MMIO 必须已正确映射并覆盖设备寿命，frame allocator 必须先初始化。

`query_info` 去掉 name 末尾 NUL；查询失败使用 `"VirtIO input"`。它检查 event type 2（REL）和 3（ABS）是否有任一能力位：name 含 keyboard 判 Keyboard；否则有 REL/ABS 或 name 含 tablet/mouse 判 Pointer；其余 Unknown。ABS 设备再查询 axis 0/1 的 min/max，失败降级为 `None`。这些降级不会使构造失败，所以诊断必须能区分“不支持”和“查询 transport 错误”。

`pop_event` 把 vendor event 的 type/code/value 原样转换，空队列返回 `Ok(None)`，不得阻塞。SYN、KEY、REL、ABS 的组合与状态机属于上层；驱动不能丢掉 SYN 或擅自合并重复事件。

## DMA 实现风险

该 HAL 与其它 VirtIO 后端一样逐页取 frame，但此版本的地址计算是普通 `ppn * PAGE_SIZE`，清零长度也是普通乘法；极值可溢出。若最终地址为空，代码返回 dangling pointer，却没有归还已取得的 frames。`dma_dealloc` 也没有 `paddr == 0`/`pages == 0` guard，若 vendor 对失败结果调用 dealloc，可能尝试释放从 PPN 0 开始的页。

多页连续性依赖栈式 allocator 恰好返回递减连续 PPN，不是原子保证。恒等映射使 `share` 可直接将 VA 当 PA；非恒等映射/IOMMU 平台必须整体替换 HAL。建议统一复用经过 checked 算术和 RAII 回滚的公共 VirtIO DMA 实现。

## 锁与事件生命周期

外层设备 mutex 只应覆盖一次短 `pop_event`，随后立即释放，再向 evdev 队列投递和唤醒。禁止持设备锁获取 evdev 队列锁后又存在反方向调用。当前没有 IRQ→waitqueue，worker 轮询频率决定延迟和 CPU 占用。

缓存的 `InputDeviceInfo` 随设备同寿命，返回引用不可跨越设备注销。当前静态平台注册通常不销毁；若加热拔插，应先从 registry 摘除并停止 worker，再等待借用/队列操作结束，最后 Drop transport DMA。

## 扩展示例

增加 gamepad 分类时应优先依据 EV_KEY/EV_ABS 的具体 capability，而非只匹配 name。扩展 `InputDeviceKind` 后同步更新 evdev 节点策略，并保留 Unknown 回退；能力查询错误不要伪造“明确不支持”。

## 回归清单

- 空/错 MMIO、错误设备类型和 negotiation 失败；
- keyboard、relative mouse、absolute tablet、Unknown、空/NUL name；
- name/ev_bits/abs_info 分别失败时的降级结果；
- 空队列、单事件、完整 SYN 帧、突发事件顺序和值符号；
- DMA 0/1/多页、OOM、非连续、地址/长度溢出及失败后的 frame 基线；
- 多设备并发轮询不发生锁反转，设备销毁不留下 worker/UAF。
