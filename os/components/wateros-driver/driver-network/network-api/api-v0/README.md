# Network Device API v0 离线开发手册

[Driver 总览](../../../README.md) · [Driver API](../../../driver-api/api-v0/README.md)

本 crate 定义同步以太网帧收发、MAC/MTU和全局 registry。IP/TCP/UDP、ARP、socket、
VirtIO queue及 IRQ调度在其他层。

## 1. 帧契约

- `mac_address` 返回稳定6字节地址；
- `mtu` 是 IP payload上限，默认1500，不含14-byte Ethernet header、VLAN和 FCS；
- `send(buf)` 的 buf是完整 L2帧，调用者填 dst/src MAC和 EtherType；
- `receive(buf)` 无包时约定返回 `Ok(0)`；有包必须一次返回完整帧；
- buffer不足应返回 `InvalidParam`，不得截断并消费该帧；
- `is_link_up=false` 时协议栈应停止发送或按策略返回错误。

API是同步/轮询式，没有 waker或 token。实现不能在持 spin mutex时睡眠等待 RX/TX IRQ；
应立即完成、返回 no-data/错误，或由外层 poll调度重试。

## 2. 调用链和锁

```text
machine probe -> VirtIO net negotiate + RX buffer预投递
  -> register_network_device(Arc<Mutex<Box<dyn NetworkDevice>>>)
network poll task
  -> clone device
  -> lock
  -> receive一批/发送队列
  -> unlock
  -> 协议栈解析、socket唤醒（锁外）
IRQ
  -> ack/标记 poll pending，不在中断内跑完整协议栈
```

不要持 device锁调用 socket/VFS/logger；它们可能反向触发发送。大包复制和协议处理应在
释放设备锁后进行。

## 3. MTU与buffer计算

普通无VLAN以太网帧常需 `mtu+14` 字节，设备可能还带 virtio-net header，但 transport
必须剥离/添加该 header，不能暴露给 NetworkDevice调用方。FCS通常由设备处理。

所有加法使用 checked arithmetic。发送前验证最小 L2 header、设备最大 frame和 negotiated
offload；当前 API无 scatter-gather/checksum/GSO元数据，所以后端应提供普通完整帧语义。

## 4. 注册表生命周期

全局 Vec只追加，无注销/去重，index稳定。getter clone Arc后释放 registry锁。Vec增长
不可失败；重复 init会重复网卡。热拔插需 generation slot，不能删除 Vec导致 `ethN`
错位。

MAC不保证 registry唯一；机器 probe应拒绝全0、全ff或按策略生成 locally administered
地址，并记录稳定设备 identity。

## 5. 当前 Sample 的已知契约错误

内存 `SampleNetworkDevice::receive` 使用 `min(self.buf.len(),buf.len())`，当目标过小时
会返回并 drain部分数据。这违反 trait注释“buffer不足返回 InvalidParam”和完整帧边界。
该 sample只能验证最小 happy path，不能作为后端模板；修复时应先比较长度，不足不消费。

sample的 send还把多次帧 append成一条 byte stream，没有帧队列边界。真实测试要用
`VecDeque<Vec<u8>>`或固定 descriptor ring表示每帧。

## 6. 新后端实例

新增 e1000：

1. 校验 PCI BAR/IRQ，分配并预投递有界 RX ring；
2. 读取/验证 MAC，确定 MTU；
3. send检查完整帧长度，填一个或多个 TX descriptor并同步等待/返回 Busy策略；
4. receive只在完整 descriptor chain ready时消费；buffer小则不消费并 InvalidParam；
5. 正确处理 DMA ownership、memory barrier和 cache coherence；
6. IRQ只 ack并唤醒 poll任务；
7. ready后一次 register，失败释放 IRQ/ring/DMA。

## 7. 回归

- 无包=0、单帧、连续多帧且边界不合并；
- RX buffer不足不消费，重试大 buffer取到同一帧；
- MTU边界、VLAN、最小帧、过大帧；
- TX/RX descriptor wrap、queue full、设备 reset、link down/up；
- 两个 poller并发、IRQ与poll竞争、锁序；
- 重复注册、多个网卡、MAC稳定；
- 长时间 flood后 DMA/heap/descriptor回基线。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

