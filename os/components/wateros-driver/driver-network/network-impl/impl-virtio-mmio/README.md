# VirtIO-MMIO Network 实现手册

[Network API](../../network-api/api-v0/README.md) · [Network Driver 总览](../../README.md) · [协议栈实现](../../../../wateros-network/network-impl/impl-smoltcp/README.md)

本 crate 将一个 MMIO VirtIO net function 包装成完整以太网帧接口。IP、ARP、TCP、socket 和 waitqueue 都在上层，不应进入设备锁内。

## 对象与初始化

`VirtioNetDevice` 内部类型为 `VirtIONet<VirtioMmioHal, MmioTransport<'static>, 32>`：队列泛型大小是 32，RX buffer 长度固定 2048 字节（包含 VirtIO net header，覆盖默认 1500 MTU）。

```text
MmioRegion -> from_mmio -> MmioTransport::new
           -> VirtIONet::new(transport, 2048)
           -> 平台注册 Arc<Mutex<dyn NetworkDevice>>
           -> smoltcp poll -> receive/send
```

零 base 返回 `InvalidDtb`，transport/feature/queue 失败为 `Unsupported`。构造前 frame allocator 与 MMIO 映射必须就绪。MAC 直接来自协商后的 config，`mtu()` 当前无条件返回 `DEFAULT_MTU`，没有读取设备 MTU feature。

## 收发精确语义

`send` 为每帧构造 `TxBuffer::from(buf)` 并同步提交，错误映射 `IoError`。调用者必须传一整帧，不能把一个帧分多次 send。

`receive` 的当前实现：

1. vendor `receive()`；NotReady 返回 `Ok(0)`；
2. 取得 `rx_buf.packet()`；
3. 若用户 buffer 过小，回收 RX buffer 后返回 `InvalidParam`；
4. 否则复制 packet，记录 `packet_len()`，回收 RX buffer，返回 `packet_len.min(buf.len())`。

第 3 步已经从设备队列消费并丢弃了该帧，违反 Network API “buffer 过小不能消费，调用者可用更大 buffer 重试”的契约。这不能只改返回码：需要驱动保存一个 `pending_rx` owner，直到成功复制后才 recycle，或在 API 增加 peek/required-length 机制。另应返回实际复制的 `packet.len()`；当前把它和 `packet_len()` 混用，虽 vendor 通常相等，语义仍不稳固。

RX buffer 的 recycle 失败只记录 warning，帧仍向上报告成功，持续失败会耗尽 RX 队列。应把设备置入可诊断错误状态或尝试恢复，不能长期静默降容。

`is_link_up()` 当前实现为 `can_recv() || can_send()`，这是队列是否可操作，不是 VirtIO link status。队列可发时即使宿主断链也可能返回 true；没有 STATUS feature 时应明确使用“设备 ready”命名，支持后则读取 config status。

## DMA 与锁

HAL 逐页取 frame、验证递减连续 PPN、清零并假定 `paddr == vaddr`。它依赖无并发插入的栈式 allocator，不是原子连续分配；`Vec` 还会消耗内核 heap。PPN 地址 checked，但 `pages * PAGE_SIZE` 未 checked。IOMMU/非恒等映射需要整体重写 share/unshare 和 cache coherency。

协议栈通常持 device mutex 调 send/receive。驱动在锁内不得睡眠、等待 socket 锁或反向进入 smoltcp。poll 循环应限制单次工作量，否则高包速率可能长期占锁。

## 扩展示例：修复 pending RX

设备结构增加 `pending_rx: Option<RxBuffer>`。receive 首先复用 pending；若目标过小，把 owner 放回 pending 并返回包含所需长度的错误/查询结果；复制成功后 recycle。Drop、reset 和 unregister 必须回收 pending buffer。由于具体 `RxBuffer` 类型含 HAL/queue 生命周期，实现前应确认 vendor 是否允许 buffer 跨调用持有；不允许时就必须扩展 API 或用驱动拥有的完整帧 bounce buffer。

## 回归清单

- 错 MMIO/type/feature、DMA OOM/非连续及构造销毁基线；
- MAC、MTU、宿主断链和 queue-ready 的区别；
- 空 RX、最小/最大帧、多帧严格顺序；
- 小 buffer 后用大 buffer 重试得到同一帧，不能丢包；
- `packet.len()`/`packet_len()` 不一致的注入；
- recycle 与 TX 失败、队列耗尽后的恢复；
- ARP、ICMP、UDP、TCP 大流量和 SMP poll 锁序。
