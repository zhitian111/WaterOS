# K-05D：ramfs 物理页后端与 tmpfs 容量边界

## 状态

核心实现已完成并进入 `main`：`OwnedPhysPage` 由
[`0ad6627a`] 引入，`impl-ramfs`
的 payload 切换由 [`dc26172d`] 完成，
bootstrap `/tmp` 默认限额为 512 MiB。2026-08-09 已在当前 `main`
复验物理页分配/回收、128 MiB 实写和 `ENOSPC` 路径，证据见
[`results/k05d-ramfs-physical-pages-20260809.md`](./results/k05d-ramfs-physical-pages-20260809.md)。
完整 pre/busybox/LTP 与 iozone 仍按 K-05 上层门禁继续补测。

## 任务性质与目标

这是 P0 资源正确性任务，不是扩大堆或只做性能调优。将 ramfs 普通文件的
实际数据页从全局内核堆转移到物理页分配器，保留稀疏文件语义，并使 `/tmp`
在页耗尽或超过 mount 限额时返回 `ENOSPC`，而不是引发 kernel heap OOM/panic。

文件树、路径、xattr 和页索引等元数据本阶段可继续使用堆；验收重点是文件 payload。

## 已知信息与证据

`impl-ramfs` 已经按 4 KiB 稀疏存储，但每个已写页仍是一个堆分配：

```rust
struct SparseFile {
    len: u64,
    pages: BTreeMap<u64, Vec<u8>>,
}
```

当前内核堆为 128 MiB，bootstrap `/tmp` 没有限额：

```rust
pub const KERNEL_HEAP_SIZE_BIT_WIDTH: usize = 27;
let fs: SharedRwFs = fs::new_ramfs_rw(None, 0o1777);
```

因此稀疏 `truncate` 不再分配 hole，但 iozone/LTP/BuildStorm 实际写入的 `/tmp`
内容仍会挤占全局堆。现有 `PhysicalFrameAllocator` 只管理帧 ID，还需要明确双架构下
物理页如何被内核安全访问、清零和回收。

## 依赖与架构约束

- 先确认 frame allocator 初始化时序，ramfs provider 只能在此之后注册。
- 在架构无关的 API 层定义窄的 page owner/page store 契约，由内核组装层注入。
- `impl-ramfs` 不得直接依赖 `impl-sv39` 或 `impl-loongarch64`，也不得制造
  FS↔MM 循环依赖。若直接使用 frame-allocator aggregate，先以 Cargo 依赖图证明无环。
- 页句柄必须是 RAII 所有者：分配后先清零，最后一个所有者销毁时恰好回收一次。
  不允许把可越过页句柄生命周期的 raw pointer 交给 ramfs。
- 页分配失败和 mount 限额超标统一映射到 `FsError::NoSpace`。不得 `unwrap`/panic，
  不得通过扩大 `KERNEL_HEAP_SIZE` 掩盖问题。
- 保留 hole 读零、写全零页可释放、shrink 释放页及清零尾部、再 grow 不暴露
  旧数据的语义。
- 不得持 ramfs 全局 spin lock 调用可阻塞的 VFS/设备 I/O。明确 frame lock、
  ramfs lock 与 inode/page lock 顺序，覆盖多核 truncate/write/unlink 竞态。

可选契约只需表达下列语义，不强制具体 trait 形式：

```rust
trait RamFsPageStore {
    type Page: RamFsPageOwner;
    fn alloc_zeroed(&self) -> FsResult<Self::Page>;
    fn with_page<R>(&self, page: &Self::Page, f: impl FnOnce(&[u8]) -> R) -> R;
    fn with_page_mut<R>(&self, page: &mut Self::Page, f: impl FnOnce(&mut [u8]) -> R) -> R;
}
```

## 涉及文件

- `os/components/wateros-fs/fs-impl/impl-ramfs/src/lib.rs`
- `os/components/wateros-fs/fs-impl/impl-ramfs/Cargo.toml`
- `os/components/wateros-fs/fs-api/api-v0/src/lib.rs`（仅在需要共享契约时）
- `os/components/wateros-fs/src/lib.rs`
- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-api/api-v0/src/lib.rs`
- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs`
- `os/src/user_bringup_root_layout.rs`
- `os/components/wateros-base/base-config/src/mm.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/`

## 实施步骤

1. 增加最小计数器，记录测试前后 heap used/free、frame used/free、ramfs resident
   pages/bytes；先用短时定向测试保留基线。
2. 定义双架构共用的页所有者和安全访问契约，在 MM 初始化后由组装层注册；
   先为 alloc/zero/read/write/drop 做独立自测。
3. 将 `SparseFile.pages` 的 `Vec<u8>` 替换为页所有者。不改变页索引和文件
   offset 语义，不为 hole 分配物理页。
4. 使 `size=` 按 resident payload 物理字节计费，对 bootstrap `/tmp` 配置显式上限
   或物理内存保留策略；限额不得计入 hole。
5. 补齐 unlink 但 fd 仍 open、hardlink、rename overwrite、truncate、mount drop 的页生命周期。
   若现有 `Node: Clone` 会深拷贝 payload，先将文件实体改为共享 inode/object 所有权，
   不能用物理页别名破坏 hardlink 语义。
6. 稳定后删除临时高频日志，仅保留低成本统计和失败日志。

## 验收方法

- [ ] `cd os && make rv_check && make la_check` 通过，两个架构的基本启动和 `/tmp`
      读写通过。
- [ ] 对 300 MiB 文件只执行 sparse truncate 时 resident frame 增量为 0；hole 读取全零。
- [ ] 在物理内存足够的 QEMU 中向 `/tmp` 实写超过 128 MiB，内核堆占用仅随
      元数据有界增长，文件数据体现为 frame 占用，无 heap OOM/panic。
- [ ] 超过 tmpfs 限额或 frame 耗尽时系统调用稳定返回 `ENOSPC`，且已有数据未损坏。
- [ ] 覆盖跨页读写、部分零写、全零页回收、shrink/grow 尾部清零和大偏移。
- [ ] unlink/open-fd/hardlink/rename/truncate/unmount 后 frame 数回到基线，无泄漏、
      double free 和 use-after-free。
- [ ] 8 核并发 write/truncate/unlink 压力通过，无锁顺序死锁和页内容串扰。
- [ ] 白天先运行定向 ramfs/tmpfs 测试和相关 LTP 用例；iozone、BuildStorm、全量
      LTP 及长时 SMP 压力留到夜间授权后执行。

结果写入 `docs/tasks/known-issues/results/k05d-YYYYMMDD.md`，包含 commit、QEMU 内存/核数、
命令、heap/frame 前后计数和原始日志路径。页契约、ramfs 转换和测试建议分成
可单独 review/回退的提交。
