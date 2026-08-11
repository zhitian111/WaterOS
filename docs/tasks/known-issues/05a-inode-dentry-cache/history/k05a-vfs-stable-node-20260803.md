# K-05A VFS 稳定节点接入报告（2026-08-03）

## 问题与目标

VFS page cache 原先按路径标识文件。hardlink 会产生互不一致的缓存；unlink 或 rename
overwrite 后，旧 fd 可能重新解析到同名新文件。为补救 unlink，旧实现还会把整个文件
复制到内核堆，且单文件上限为 16 MiB。目标是让打开文件的所有 I/O 使用稳定对象身份，
同时保留不支持稳定节点的文件系统回退路径。

## 实现

- `PagedFileHandle` 打开时调用 FS `api-v0::ReadWriteFs::open_node()`，并用
  `mount identity + FsNodeId` 构造 page-cache key；mount generation 仍由独立的全局
  cache 实例隔离。
- read/write、prepared read、metadata、truncate、flush、clone 和 open-ref 计数统一
  使用稳定 key；`FsPageIo` 对稳定节点直接调用 node range I/O。
- hardlink 的不同路径因 inode 相同而共享脏页；rename 只更新展示路径，不改变对象 key。
- 稳定后端 unlink 不再创建 detached `Vec<u8>` 快照。another-ext4 保留 inode，最后一个
  node lease 释放时清理内部硬链接。
- unlink 后脏页写回完成后直接同步 lease 所属 `SharedRwFs`，不再按已经消失的路径路由。
- 未实现 stable-node API 的后端继续使用原有 path/detached 兼容路径。

涉及文件：

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs`

task 调度器及其 API 未修改；VFS 没有依赖 another-ext4 私有类型。

## 验证结果

- `make rv_check`：通过。
- `make la_check`：通过。
- page-cache host tests：13 passed。
- 新版决赛 RISC-V 镜像，8 核 QEMU：`STABLE_NODE_REGRESSION_DONE`，exit 0，
  约 3.42 秒。
- 初赛 RISC-V 镜像，8 核 QEMU：同一回归 exit 0，约 2.25 秒。
- 两个 overlay 的 `e2fsck -fn` 均通过五阶段检查。
- 决赛 overlay 的 `/.wateros-open-inodes` 没有有效遗留链接。

定向回归覆盖 hardlink 脏页可见性、unlink 后继续读写、同名重建隔离、rename overwrite
旧 fd、17 MiB 文件 unlink 后尾部读取，以及 close/sync 后 orphan 清理。日志：
`/tmp/wateros-k05a-new-rv.log`、`/tmp/wateros-k05a-pre.log`。

新版主办方 RISC-V 压缩包 SHA-256：
`cba87f43ae569bcf2b8e4614f75cec1bf51bedb2804626fe466fcce3861df6f1`。工作镜像先执行了
journal recovery；规范化 raw SHA-256：
`61d1fb20a61d2af1bf2d1e7c8d0031eb0c867bb6599bd659b41465c7cf420926`。

## 剩余验收

本提交证明稳定身份语义和镜像一致性，但不宣称 K-05A 整体性能验收完成。仍需运行三轮
stat/open/read 对比、LTP FS 子集以及新版镜像上的完整 CAgent/BuildStorm；这些结果进入
后续独立报告，发现的新问题不回填为本提交已通过。
