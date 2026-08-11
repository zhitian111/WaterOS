# K01 页缓存稳定节点写回修复报告

## 结论

全局页缓存的并发写入和淘汰路径存在跨文件写回错误。该问题会破坏普通文件内容和 ext4 extent 元数据，并能解释 BuildStorm 中 guest `rustc` 的随机 `SIGSEGV`。本次已完成定向修复和一致性验证；完整 BuildStorm 仍需在修复后的干净覆盖盘上复测。

## 根因与修复

涉及文件：

- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`

修复内容：

1. 写页面时，在同一临界区发布 frame dirty version、`dirty_pages` 和 `logical_size`，避免淘汰线程用旧 EOF 丢弃扩展写。
2. `FsPageIo` 不再把所有淘汰写回都发送到当前句柄 inode。新增按 `(mount_generation, cache_key)` 索引的稳定节点路由，使全局缓存淘汰其他文件时写入正确 inode。
3. 为直接丢弃的 `PagedFileHandle` 增加 `Drop` 收尾，执行 dirty sync 并释放 open ref；显式 `close()` 后不会重复执行。
4. 稳定节点表每 256 次注册清理失效 `Weak` 项，避免编译工作负载打开大量文件时持续增长。

## 故障证据

修复前的 5 路文件压力测试中，目标文件开头出现其他压力流的内容，目标哈希错误；离线 `e2fsck -fn` 同时报告 extent tree 结构错误。继续追踪发现，VFS 自测直接 drop 句柄后还会留下 `/vfs_at_io_smoke` 的脏页路由状态。

此前完整 BuildStorm 已运行到 `compiler_builtins` 编译，但 guest `rustc` 收到 `SIGSEGV`；覆盖盘中的构建输出也出现异常稀疏区。该结果作为修复前基线，不视为兼容性失败结论。

## 验收结果

- RISC-V QEMU，4 路各 32 MiB 压力写入并并行写入 16 MiB 目标文件：通过。
- 目标文件：`16777216` bytes、`32768` blocks，SHA-256 为 `98cc801f342a6397d28d177b78766b5d73fa808c033ffb8cd49257baabdfdb89`，与预期一致。
- 覆盖盘离线 `e2fsck -fn`：5 个检查阶段全部通过；仅有可选的 extent tree 压缩提示，无文件系统错误。
- `cargo test -p wateros-vfs-impl-page-cache`：13 passed。
- `make rv_check`：通过。
- `make la_check`：通过。

## 后续

必须基于主办方新镜像创建全新的 qcow2 覆盖盘，重新运行完整 BuildStorm。验收标准是编译任务正常结束、无 guest fault/`SIGSEGV`，并在关机后再次执行离线 `e2fsck -fn`。
