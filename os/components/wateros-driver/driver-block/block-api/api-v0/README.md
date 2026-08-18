# Block Device API v0 离线开发手册

[Driver 总览](../../../README.md) · [Driver API](../../../driver-api/api-v0/README.md)

本 crate 定义同步块 I/O、LBA、共享句柄和全局注册表。transport queue、DMA、缓存、分区和
文件系统不在此层。

## 1. 数据与调用链

```text
machine probe -> VirtIO block 构造完成
  -> Arc<Mutex<Box<dyn BlockDevice>>>
  -> register_block_device -> 稳定 index
  -> rootfs/block cache clone SharedBlockDevice
  -> device mutex -> read_blocks/write_blocks/flush
```

`Lba(u64)` 从 0 开始，单位由该设备的 `block_size()` 决定，不能跨不同块大小设备直接
复用。当前默认 `BLOCK_SIZE=512`，但 trait 允许覆盖。

`SharedBlockDevice` 的外层 spin mutex同时串行 VirtIO queue状态和设备操作。调用者不能
持该锁跨 VFS锁、睡眠或 console输出；先完成 I/O，再释放 guard并处理上层状态。

## 2. trait 契约

- `block_size` 必须非零且生命周期内稳定；
- `total_blocks` 的 `None` 是未知容量，不是 0；
- `read_blocks/write_blocks` buffer长度必须为 block size整数倍；
- 请求范围为 `[start_lba,start_lba+len/block_size)`；
- 成功必须完成整个 buffer，不能用短读/短写表示部分完成；
- readonly设备的 write 返回 `Unsupported`；
- `flush` 提交此前接受的写到稳定介质；没有易失缓存的实现可以成功空操作。

`check_request_range` 检查块大小、整块长度、LBA加法和已知容量。零字节请求会形成 0
块；已知容量时仍检查 start不超过 capacity。具体 backend必须在提交 descriptor前调用，
默认 read/write trait不会自动调用。

## 3. 默认字节读取

```text
read_bytes(offset,dst)
  -> offset 转 usize，checked offset+len
  -> floor/ceil算覆盖块
  -> vec![0; block_count*block_size]
  -> read_blocks(start_lba, scratch)
  -> 拷贝页内子区间到 dst
```

空 dst 不访问设备。该实现会为整个覆盖范围进行**不可失败 Vec分配**；大 read可能触发
内核 heap allocation panic，而 `DriverError` 又没有 OOM。性能路径应使用有界 scratch
逐块循环或调用者提供缓冲，不能用 `read_prefix` 读取巨大镜像。

没有默认 `write_bytes`，因为非对齐写需要 read-modify-write、并发序列化和崩溃一致性；
应由 block cache提供，而不是随意加一个不原子的 helper。

## 4. 注册表生命周期

全局 `Mutex<Vec<SharedBlockDevice>>` 只追加，无去重、注销或热拔插。返回 index在本次
启动期间稳定。getter在 registry锁内 clone Arc 后立即解锁，不会嵌套持有设备锁。

注册 Vec增长是不可失败分配；OOM会 panic。重复 init会重复发布同一盘，所以 machine init
guard必须在 register之前生效。热拔插需要新的 slot generation/tombstone协议，不能从 Vec
删除导致 index漂移。

## 5. 新后端实例

实现 NVMe namespace 时：

1. 完成 PCI BAR/queue/DMA init后构造对象；
2. `block_size/total_blocks` 读取 namespace几何并固定；
3. 每次 I/O先 `check_request_range`，再把大请求拆成 controller允许的 descriptor；
4. 同步 API需等待 completion，但不能在关中断且依赖同 CPU IRQ时死等；
5. status错误映射为 InvalidParam/IoError/Unsupported；
6. flush映射 NVMe Flush，成功后才返回；
7. 全部 ready后一次 register，失败释放 queue/DMA/IRQ。

## 6. 已知边界与测试

- API无 discard、write-zeroes、barrier、FUA、只读标记或异步 completion；
- registry无设备 identity，顺序依赖枚举顺序，根盘最好按稳定设备信息选择；
- `SampleBlockDevice` 只覆盖两块内存和 readonly行为，不测并发/flush持久性；
- block cache与设备锁的锁序必须由上层文档定义。

回归：0/非整块长度、最后一块、越界、LBA溢出、未知容量、跨块字节读、backend短完成、
flush失败、并发读写、重复注册和 heap压力。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

