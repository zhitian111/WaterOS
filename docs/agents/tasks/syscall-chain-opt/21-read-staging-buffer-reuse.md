# 任务 21：复用 read staging buffer

## 任务内容与目标

降低普通文件 read 每次 `try_zeroed(Vec)`、填充和二次复制的分配/清零成本，同时保留用户页
故障、部分成功、文件 offset reservation 和“锁内不访问用户内存”的安全语义。

## 实施方案

1. 优先实现有界 per-CPU/size-class staging pool；buffer 返回池前清理可能泄漏的数据范围。
2. 不跨任务保留可观察旧数据；内存压力下可回退正常分配，不阻塞等待 buffer。
3. 读取只初始化实际返回区间；Rust 初始化安全不能用未证明的 `set_len`/裸切片绕过。
4. `VfsReadLease::finish` 在 EFAULT 时提交准确 copied 字节和 offset，buffer 必须可靠归还。
5. direct-to-user 仅在能证明 page pin/fault 与锁顺序安全后作为后续方案，不混入本提交。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- VFS read lease/staging 辅助模块
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `fallible_buf.rs` 或现有内核 buffer allocator

## CodeGraph 查询

```bash
codegraph explore "PagedPreparedRead StagedReadLease try_zeroed finish_scattered_read"
codegraph impact "VfsReadLease"
codegraph callers "try_zeroed"
```

## 验收方式

```bash
cd os
make rv_check && make la_check && make kernel-rv-final
# 短读、EOF、EFAULT、并发 read、不同 size class、内存压力回归
cd .. && git diff --check
```

增加测试证明跨进程无数据泄漏、EFAULT offset 正确、pool 有界。任务 00 runner记录分配次数、
清零字节与 BuildStorm A/B；无稳定收益则回退 pool，保留简报。

## Commit 与简报

提交建议：`[perf] 复用普通文件 read staging buffer`。新增 `history/21-brief.md`。
