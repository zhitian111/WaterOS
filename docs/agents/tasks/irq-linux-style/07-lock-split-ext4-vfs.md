# T07：ext4/block-cache/VFS 锁拆分

## 任务内容

这是性能能否成立的关键一步。目标是消除「跨 I/O 持锁」：

```text
SharedRwFs spin lock
  -> another_ext4 全局 block-cache spin lock
    -> SharedBlockDevice spin lock
      -> virtio add_notify_wait_pop busy poll
```

要求 miss 时释放缓存/FS 锁，提交请求并睡眠，唤醒后回查再装填；绝不在持有
`spin` 锁的情况下 sleep 或切换任务。

## 实施方案

1. 块缓存：把元数据/数据缓存锁与 backend 调用分离；miss 释放锁 → 读 backend →
   重取锁 → 二次检查后装填。
2. `another_ext4`（vendor 补丁，需明确理由）的 block-cache 锁同样在 backend read 前释放并回查。
3. `SharedRwFs` 从互斥锁改为读写锁（或等价），读侧并发；变更操作保持独占。
4. 先做纯重构不引入 IRQ，用同步 backend 验证锁语义正确；T06 的异步路径再叠加。
5. 按 `os/components/wateros-vfs/README.md` 已有 lock ordering 文档更新锁序说明。

## 涉及文件

- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/{lib,device,manager}.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`
- `os/vendor/another_ext4/src/ext4_defs/cache.rs`（vendor 补丁，谨慎）
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{lib,paged_handle,mount_table,path_ops}.rs`
- `os/components/wateros-fs/fs-api/api-v0/src/lib.rs`（`SharedRwFs`/`SharedFs` 类型）

## CodeGraph 查询命令

```bash
codegraph explore "SharedRwFs LocalRwFs ReadWriteFs SharedBlockDevice"
codegraph impact SharedRwFs
codegraph explore "CachingBlockDevice read_blocks admit_read_miss"
codegraph explore "FsPageIo read_range write_range"
```

## 验收方式

- 静态：双架构 check。
- 单测：块缓存并发 miss 回查与一致性 host test；another_ext4 `--features block_cache` test。
- QEMU 功能：双架构 rootfs 挂载、读写、重开、fsync/unmount 后 `e2fsck -fn`（镜像副本）无错。
- 重点观察无死锁、无饥饿、无数据损坏。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
# 用镜像副本 + overlay 验证，避免污染基准镜像
```

## 任务简报

完成后写 `history/07-lock-split-ext4-vfs.md`。
