# K-05 close 写回与全局 sync 分层结果

## 问题

BuildStorm 的 GDB 快照显示多个编译任务同时位于 `FsPageIo::write_range()`，并竞争
another-ext4 的全局锁。继续审计发现 `PagedFileHandle::close()`/`Drop` 在写回当前文件
脏页后还调用 `ReadWriteFs::sync()`；another-ext4 的实现是 `Ext4::flush_all()`。因此每
关闭一个编译产物都会扫描并写回整个 ext4 块缓存。

此外，原 `sync(2)` 逐个调用打开 fd 的 `flush()`，可能重复执行多次全文件系统同步；
已关闭文件只留在 ext4 写回缓存时，反而没有统一的根文件系统同步边界。

## 修复

- 将 `PagedFileHandle` 的文件页写回与持久化同步分离。
- `close`、`Drop` 和 truncate 前置处理只写回当前文件，继续报告写回错误，不再隐式
  执行全文件系统 `fsync`。
- `fsync`/`fdatasync` 仍先写回当前文件，再同步底层文件系统；没有 VFS 脏页时也会
  提交已有文件系统元数据。
- 新增 `sync_file_page_cache()`：一次性写回全局文件页缓存并同步根文件系统，不清空
  热缓存；原 cache reset 复用该入口。
- `sync(2)` 直接调用一次全局同步，不再按打开 fd 重复执行 `flush_all()`。

未修改 task 模块、调度接口、FS API trait 或 another-ext4 vendor 代码。

## 验证

- `make check`、`make la_check`：通过。
- `cargo test -p wateros-vfs-impl-page-cache`：13 项全部通过。
- RISC-V64/OpenSBI/QEMU 8 核初赛定向 LTP：`close01`、`close02`、`fsync02`、
  `fsync03`、`fdatasync01`、`fdatasync02` 全部退出 0，无 `TFAIL`/panic/OOM。
- `fsync01`、`fsync04`、`fdatasync03` 需要 LTP 独占测试块设备，当前镜像无可分配设备，
  因环境限制报告 `TBROK`，未作为内核失败。
- 精确最终实现上运行 `close01/02` 后执行 BusyBox `sync`：全部退出 0。
- 非 snapshot 写入并关机后，`e2fsck -fn` 五阶段通过。
- final 短程回归：CAgent 10/10、BuildStorm toolchain/minibuild 通过；`tg-xtask`
  预构建 `1m00s`，内层 dev 构建 `1m20s`，进入正式并行编译且无异常。

日志：

- `/tmp/wateros-close-sync-rv-final.log`
- `/tmp/wateros-close-sync-final-persist-rv.log`
- `/tmp/wateros-rv-buildstorm-close-writeback-short.log`

本次不宣称短程 BuildStorm 有可测加速：前置阶段与修复前 `1m01s`/`1m18s` 基本相同，
主要收益位于大量编译产物关闭和最终同步阶段。修复后的完整 BuildStorm 仍作为后续 final
总门禁执行；修复前一提交已在同一主办方镜像完整通过并产出结果。
