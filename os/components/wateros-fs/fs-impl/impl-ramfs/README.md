# impl-ramfs

[返回 wateros-fs](../../README.md) · [VFS](../../../wateros-vfs/README.md) · [MM](../../../wateros-mm/README.md)

ramfs 是内存中的 `ReadWriteFs` backend，供 VFS tmpfs 策略层使用。它拥有 inode/tree、目录项和 sparse file payload，不拥有 mount namespace、per-task fd 或 syscall tmpfs flag。

## 数据结构

[`tree.rs`](src/tree.rs) 的主要层次：

```text
RamFs
  -> nodes / inode allocator / byte limit accounting
  -> Node
       -> Inode metadata
       -> NodeKind
            Directory(children)
            File(SparseFile)
            Symlink(target)
  -> open node bookkeeping
```

`SparseFile` 只为实际写入区间持有数据块，文件逻辑 size 与已分配 payload 不同。truncate 扩大只改逻辑长度，空洞读取返回零；truncate 缩小必须释放超出范围 payload 并更新配额。

`limit_bytes: Option<usize>` 是实例级容量策略。所有会增长 payload/metadata 的路径应先做 checked 计算和配额验证，再提交 tree 变化；不能修改一半后返回 NoSpace。

## 操作与锁边界

[`operations.rs`](src/tree/operations.rs) 实现 `ReadWriteFs`。调用方通过外层 `SharedRwFs = Arc<Mutex<...>>` 串行访问实例，因此内部对象不应泄漏跨解锁的裸引用。

路径操作必须维护：父目录类型、名称合法性、inode/nlink、目录项、mtime/ctime、打开引用和配额。rename 是最复杂事务：先验证 source/destination、目录环和替换规则，再一次提交；失败不能丢 source。

open handle 的 identity 应在 unlink/rename 后仍指向同一 inode。最后 link 与 open 引用消失时才回收节点/payload。

## 与 tmpfs 的边界

VFS 决定 mount point、`tmpfs` 名称、mount flags 和 per-namespace 可见性；ramfs 只提供一个新的共享 RW 文件系统实例：

```text
VFS mount tmpfs
  -> fs::new_ramfs_rw(limit_bytes, root_mode)
  -> 新 RamFs 实例
  -> mount table 安装到 namespace
```

不同 mount 应有不同 RamFs 实例，除非明确要求共享。root mode 在创建时设置，后续权限检查由 VFS/credential 与 FS metadata 共同完成。

## 回归

覆盖 create/read/write、空洞、跨块、truncate 扩大/缩小、配额刚好/超限、mkdir/rmdir、hard link、symlink、rename 覆盖与目录环、open-unlink、并发 handle、多个独立 mount。压力结束后 payload 与 inode 数应回落；逻辑文件 size 不能被误算成全部已分配内核 heap。
