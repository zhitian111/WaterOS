# K-01：BuildStorm 与文件系统持久化闭环

## 任务目标

让正式 BuildStorm 完整成功，并证明 `fsync/fdatasync`、页缓存、another-ext4 与
rename/unlink/truncate 使用一致的持久化语义。最终必须输出
`BUILDSTORM_COMPILE mode=multi ok=true`，且测试后 ext4 结构完整。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-fs.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/features/wateros-driver.md`
- `docs/tasks/buildstorm-cargo-index-filesystem-report.md`
- `docs/todo/perf-fs-vfs.md`

## 已知信息与代码证据

- 大于 4 MiB 的 Cargo sparse-index 读取问题已定位为 syscall 短读错误，镜像和
  another-ext4 基本读取不是根因。
- 最新记录只证明 446 个编译单元已经开始编译，没有证明完整成功。
- 运行仍出现 `fsync fd=6 flush failed: Unsupported`，当前 syscall 在调用
  `flush()` 前硬拒绝所有非普通文件：

```rust
let meta = handle.metadata()?;
if meta.node_type != VfsNodeType::File {
    return Err(vfs::api::VfsError::Unsupported);
}
handle.flush()
```

- `VfsMetadata`/`FsMetadata` 尚未承载持久化 atime/mtime/ctime；`stat_times.rs`
  只是本次启动中的 syscall 层覆盖。这可能影响 Cargo freshness，但目前是待实验
  风险。
- page cache 已有 dirty version 和批量 flush；近期提交修复了目录增长、同父目录
  rename 和 eviction 失败隔离。不能把这些提交等同于所有并发写回已闭环。
- 2026-07-31 已完成目录 `fsync` 的句柄能力分发：another-ext4 真实执行
  `flush_all()`，非持久化句柄默认返回不支持。双架构 check、CAgent 10/10、
  BuildStorm toolchain/minibuild 和 overlay `e2fsck -fn` 已通过；完整编译仍待闭环。
  详见 [`results/k01-20260731.md`](./results/k01-20260731.md)。
- 2026-07-31 已修复 `write/writev/pwrite64/pwritev` 将 4 MiB 内核暂存上限错误暴露为
  ABI `EINVAL` 的问题。BuildStorm 已越过原先 `libc` 写入 `lib.rmeta` 的失败点并开始
  编译其依赖项；完整 BuildStorm 和崩溃一致性仍未闭环。详见
  [`results/k01-large-write-20260731.md`](./results/k01-large-write-20260731.md)。
- 2026-08-01 修复页缓存脏 victim 在底层写回失败时被提前分离并丢弃的问题。失败后
  key/data/dirty/version 保持可重试，并发更新通过 version 检查避免被旧 snapshot
  错误清理。详见
  [`results/k01-page-cache-eviction-failure-20260801.md`](./results/k01-page-cache-eviction-failure-20260801.md)。
- 2026-08-01 修复 unlink 后打开 fd 与同名新文件发生 path-key cache 别名的问题；
  旧句柄切换到 unlink 前一致快照，独立 open 继续共享旧对象，失败的 unlink 不再
  清除缓存。同时补齐中间分量非目录的 `ENOTDIR`。详见
  [`results/k01-open-unlink-identity-20260801.md`](./results/k01-open-unlink-identity-20260801.md)。
- 2026-08-01 补齐 another-ext4 不直接支持的 rename overwrite，并让源对象旧 fd
  跟随到新路径、被覆盖目标旧 fd 保持独立；页缓存 path key 与 open-ref 同步迁移。
  详见 [`results/k01-rename-replacement-20260801.md`](./results/k01-rename-replacement-20260801.md)。
- 2026-08-01 将 ramfs 普通文件改为按 4 KiB 页分配的稀疏存储，修复 LTP 创建
  300 MiB 测试设备文件时的内核堆 OOM。定向测试已推进到申请 loop/test device，
  当前剩余阻塞是 `No free devices found`，不能视为 `rename01` 已通过。详见
  [`results/k01-ramfs-sparse-files-20260801.md`](./results/k01-ramfs-sparse-files-20260801.md)。
- 2026-08-01 修复 fs-bridge 将 `O_RDONLY|O_CREAT` 错误拒绝为 `EINVAL` 的问题，
  创建动作与返回 fd 的写权限现已分离。LTP `open11`、`ftruncate03` 及相关同步、
  truncate、并发 rename 定向回归共 44 个断言通过。详见
  [`results/k01-open-readonly-create-20260801.md`](./results/k01-open-readonly-create-20260801.md)。

## 涉及文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/sync.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/stat_times.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/{handle,meta}.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{file_handle,dir_handle,paged_handle}.rs`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`
- `os/components/wateros-fs/fs-api/api-v0/src/lib.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`
- `os/vendor/another_ext4/`（只读调查；默认不得修改）
- `os/src/user_bringup_busybox.rs`
- `os/scripts/rv_final_run.sh`
- `docs/tasks/buildstorm-cargo-index-filesystem-report.md`

## 任务内容

1. 用全新 overlay 运行 BuildStorm，记录第一个真实失败点，禁止以 15 分钟诊断超时
   代替脚本结果。
2. 临时记录 `fsync/fdatasync` 的 task、fd、节点类型、路径和句柄实现，确认 fd=6 的
   类型；取得结论后删除热路径日志。
3. 在 VFS API 表达每类句柄的 flush 能力。普通文件必须真实写回；目录按 Linux 和
   后端能力返回；pipe/socket/device 不得伪造持久化成功。
4. 把 dirty-page 提交、底层 `write_range`、metadata 更新和 `flush_all` 的失败传播
   设计为一个闭环。unlink、rename、truncate、mount generation 变化时不能静默丢脏
   页，也不能把旧 inode/path 的页写到新对象。
5. 先做两次连续 Cargo build 和重启后复用 target 的实验。只有日志证明时间戳造成
   dirty rebuild 或 stat 语义失败，才扩展 `FsMetadata`、`VfsMetadata` 和
   another-ext4 映射；不得无证据扩大公共 API。
6. 为 rename/unlink/truncate 与并发 fsync 增加定向压力测试，并用相同镜像重复
   iozone/lmbench/BuildStorm。

建议 flush 能力由句柄实现决定，syscall 只做 fd/access 校验和 errno 映射，例如：

```rust
match handle.flush() {
    Ok(()) => UserRet::from_success(0),
    Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
}
```

接口名可调整，但不能在 syscall 里维护文件系统实现白名单。

## 架构约束

- 公共 metadata/flush 契约放 `wateros-fs` 或 `wateros-vfs` 的 `api-v0`。
- another-ext4 适配和错误映射放 `impl-another-ext4`，不要把 DragonOS 类型泄漏到
  VFS。
- page cache 不得持自旋锁执行块 I/O；snapshot/version 检查后锁外写回，再以版本
  确认提交。
- 不允许通过让所有 `fsync` 返回 0、禁用缓存或修改评测脚本掩盖问题。

## 如何验收

- [ ] `make rv_check`、`make la_check` 和两个 final kernel 构建成功。
- [ ] `cargo metadata --offline` 成功，`web-sys` inode、大小和 SHA-256 不变。
- [ ] BuildStorm 输出 toolchain、minibuild 和完整 compile 三个成功标记。
- [ ] fd=6 的类型和 Linux 对照结论记录清楚，正式日志中无未解释 flush 警告。
- [ ] 两次连续构建和重启复用实验得到明确时间戳结论。
- [ ] rename/unlink/truncate/fsync 并发测试无丢数据、旧页写回或 panic。
- [ ] 每轮 overlay 的 `e2fsck -fn` 五阶段通过，原始镜像 hash 不变。
- [ ] CAgent 三轮 10/10，初赛 basic/busybox 无新增回归。

结果写入 `docs/tasks/known-issues/results/k01-YYYYMMDD.md`，包含 commit、镜像 hash、
QEMU 参数、命令、耗时、首个失败点和日志路径。
