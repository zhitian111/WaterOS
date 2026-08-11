# K-05A another_ext4 稳定节点后端阶段报告

```text
task: K-05A another_ext4 stable node backend
date: 2026-08-02
base_commit: 045a904e
scope: another_ext4 adapter; VFS adoption pending
vendor_modified: no
task_architecture_changed: no
```

## 结论

another_ext4 适配层已经实现 `FsNodeId` 的 open/close、metadata、range read/write 和
truncate。打开后的 I/O 可直接使用 inode，不再依赖路径重新 lookup；后端按 inode
维护 open 引用计数。

## Unlink 生命周期

vendor 的 `unlink()` 会在最后一个目录链接消失时立即释放 inode。为保持 open fd
语义，适配层只在“仍有 open 引用的 inode 即将 unlink”时执行：

1. 在 `/.wateros-open-inodes` 建立内部硬链接并同步。
2. 删除用户路径，此时 inode 仍由内部链接持有。
3. 最后一个 `close_node()` 删除内部链接并同步，inode 才被释放。

该方案使用 ext4 正常链接计数，不修改 vendor 释放算法；普通 open/read/close 不产生
额外磁盘元数据写。RW mount 会清理上次异常关机遗留的内部链接。

## 涉及文件

- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`

## 验证

- another_ext4 组件测试：3/3 通过，包含 open 引用精确关闭回归。
- `cd os && make rv_check`：通过。
- `cd os && make la_check`：通过。
- 未修改 task、VFS API 或 `os/vendor/another_ext4`。

## 剩余风险和后续

本阶段代码尚未被 VFS 运行时句柄调用，因此 unlink 保留链路的 QEMU 行为验证放在下一
阶段一起完成。VFS 必须让 duplicate 句柄共享同一个 lease，并以
`(mount identity, FsNodeId)` 作为页缓存身份；否则 hardlink 仍可能形成两个不一致的
路径缓存。完成接入前，K-05A 总任务保持开放。
