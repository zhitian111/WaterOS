# 任务 19：建立根 FS 稳定只读并发契约

## 任务内容与目标

为 metadata、read_range_node 等稳定 inode 只读操作建立独立并发契约，使下一任务可以绕开
`Arc<Mutex<LocalRwFs>>`；写操作、目录变更、journal 与同步仍串行。本提交只建立 API、锁
不变量和 another-ext4 实现，不切换 VFS 热路径。

## 实施方案

1. 先证明 another_ext4 的 `&self` read/getattr 是否会修改内部 cache；不能仅把 Mutex 换 RwLock。
2. 将必须可变的 inode/block cache 分离到内部细粒度锁，或提供由后端保证 Sync 的稳定 read
   channel；在 API 中明确 Send/Sync、rename/unlink 和 open ref 不变量。
3. 写/journal 继续持独占锁；定义与 read channel、block device、page cache 的锁顺序。
4. 禁止在 read lock 内调度、用户拷贝或重入 VFS。
5. 增加多线程并发 metadata/read 与 rename/write/sync 的 host/内核自检。

## 涉及文件

- `os/components/wateros-fs/fs-api/api-v0/src/{handles,traits}.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/`
- 必要时 `os/vendor/another_ext4`；只有适配层无法表达时才允许 vendor patch并说明原因
- FS/rootfs 并发文档

## CodeGraph 查询

```bash
codegraph explore "SharedRwFs LocalRwFs metadata_node read_range_node another_ext4 Ext4"
codegraph impact "SharedRwFs"
codegraph callers "metadata_node"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml
make rv_check && make la_check && make kernel-rv-final
# 并发 read/metadata + write/rename/sync 压力测试
cd .. && git diff --check
```

TSAN 不适用于 no_std 内核时，必须用锁顺序诊断与 SMP 压测补证。API 引入但尚未被 VFS 使用，
因此本任务不以性能收益为条件；任何数据 race 或 vendor invariant 不清楚则停止在本任务。

## Commit 与简报

提交建议：`[refactor] 建立根 FS 稳定只读并发通道`。新增 `history/19-brief.md`，说明是否修改
vendor、锁顺序和并发证据。
