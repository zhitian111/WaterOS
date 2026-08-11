# K-05C：连续 I/O 合并与顺序预取

## 任务目标

在 K-04 证明设备块数或同步预取放大后，合并连续 ext4/block I/O，并让预取只服务
顺序访问。该任务与 K-05A/K-05B 可并行开发，但基准提交必须独立。

## 执行前必读

- `docs/tasks/known-issues/05-fs-vfs-performance/task.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-driver.md`
- `docs/exports/features/wateros-fs.md`
- `docs/exports/features/wateros-vfs.md`

## 已知信息与代码证据

block cache 已启用且容量 1024，page cache 已有顺序预取。旧“缓存未启用”结论失效；
本任务应先比较逻辑字节与设备 block 操作数：

```text
read_amplification = device_bytes / userspace_read_bytes
```

## 涉及文件

- `os/components/wateros-driver/driver-block/`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-base/base-config/src/fs.rs`

## 任务内容

1. 统计连续 LBA run、平均请求大小、预取命中/浪费和 cache 命中。
2. 在通用 block/FS API 表达批量能力，不把 QEMU virtio 细节放进 another-ext4。
3. 合并连续块读写，设置请求上限和部分完成/错误传播。
4. 以 file identity+offset 跟踪顺序流；随机/seek/多 reader 时及时停预取。
5. 保持 write-through、dirty ordering 和 K-01 flush 契约。

## 如何验收

- [ ] 设备操作数/read amplification 下降，三轮 iozone 有稳定收益。
- [ ] 随机读不因预取明显退化，内存和队列有界。
- [ ] short I/O、EOF、错误传播和并发 truncate 测试通过。
- [ ] BuildStorm、FS LTP、`e2fsck -fn` 与双架构 check 通过。

交付 `docs/tasks/history/known-issues/k05c-YYYYMMDD.md`。
