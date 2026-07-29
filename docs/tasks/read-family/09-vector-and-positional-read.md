# RIO-09：向量与定位读取收敛

## 任务目标

让 `readv(2)`、`pread64(2)` 和 `preadv(2)` 复用 RIO-01 至 RIO-08 建立的访问校验、
user-copy 进度和读取租约，统一 iovec、短读、部分成功、offset 和内存上限语义。

## 前置条件

- RIO-01、RIO-02 和 RIO-04 已合入。
- RIO-05 至 RIO-08 已为所有实际读取源实现统一 lease；iovec 解析部分可提前开发，
  但不能在部分 source 继续走旧破坏性 `read()` 时合入最终 syscall。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/public-api/wateros-syscall.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/public-api/wateros-vfs.md`
- `docs/exports/impl-guide/wateros-vfs.md`
- `docs/exports/features/wateros-mm.md`
- `docs/exports/public-api/wateros-mm.md`
- `docs/tasks/read-family/README.md`
- `docs/tasks/read-family/01-read-access-and-errors.md`
- `docs/tasks/read-family/02-user-copy-progress.md`
- `docs/tasks/read-family/04-vfs-read-lease-and-files.md`

## 已知信息与代码证据

`readv()` 当前对单个大 iovec 直接失败：

```rust
if iov.len > SYSCALL_IO_MAX {
    return UserRet::from_error(ErrNo::EINVAL);
}
```

iovec 地址使用未检查加法：

```rust
copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size)
```

如果前几个 iovec 已成功读取，而后一个 base 无效，部分路径仍直接返回 `EFAULT`。用户
buffer fault 发生前，底层 source 已经被破坏性读取。

`pread64()` 按用户长度直接分配：

```rust
let mut kbuf = Vec::with_capacity(len);
kbuf.resize(len, 0);
```

`preadv()` 又把多个 64 KiB chunk 全部 `extend_from_slice` 到一个 `gathered Vec`，
可能持有接近 `MAX_RW_COUNT` 的内核内存，并在每次调用打印 `info!`。

## 涉及文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/fallible_buf.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/user_copy.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/vfs_util.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`
- `os/components/wateros-vfs/src/fd.rs`
- syscall ABI 号和 dispatch 文件（确认是否存在 preadv2；当前未见正式实现）

## 任务内容

将 iovec 导入、逐段读取、定位读取和部分成功返回收敛到共同执行器。不得为完整请求
分配同等大小的内核缓冲区，且 `pread*` 不得修改共享 OFD offset。

### iovec 导入

先完整复制 iovec descriptor 数组到内核小结构，再开始 source read：

```rust
struct ImportedIoVec {
    base: usize,
    len: usize,
}
```

要求：

- `iovcnt > IOV_MAX(1024)` 返回 `EINVAL`。
- `iovcnt == 0` 仍先验证 fd/access；宿主 Linux `readv(-1, [], 0)` 返回 `EBADF`。
- `iov_ptr + i * size` 全部使用 `checked_mul/checked_add`。
- descriptor 数组不可访问时，在消费 source 前返回 `EFAULT`。
- 每个 zero-length iovec 允许 base 为 NULL。
- 总长度使用 checked add；大于 `MAX_RW_COUNT(0x7ffff000)` 时把可传输长度限制在
  `MAX_RW_COUNT`，不能因超过内部 4 MiB 上限返回 `EINVAL`。
- 使用 fallible `try_reserve`，OOM 返回 `ENOMEM`，不 panic。

## readv 执行

一次 source lease 最多 staging：

```text
min(total_iov_len, MAX_RW_COUNT, SYSCALL_IO_MAX)
```

然后按 iovec 顺序调用 `copy_to_user_progress`：

- 完整复制继续下一 iovec；
- 后段 fault 且此前 `copied > 0`，让 lease 提交该前缀并返回已复制总数；
- 首字节 fault，lease cancel 并返回 `EFAULT`；
- source short read/EOF 只 scatter 实际 staging 字节；
- stream、record、eventfd 的 partial commit 差异由 lease `finish` 决定，不在
  `readv` 复制 source-specific 条件。

不要逐 iovec 调一次底层 `read_fd()`；这样会改变 pipe/socket blocking 和 packet
边界。应先取得一个 lease，再 scatter。

## pread64/preadv

- 先做 fd/access/type 校验，再处理 zero length 和 pointer。
- 负 offset 返回 `EINVAL`；不可 seek 的 pipe/socket/eventfd 返回 `ESPIPE`。
- positional read 不修改共享 OFD offset。
- staging 使用 fallible、内部 capped buffer；合法大请求返回短读。
- `preadv` 直接把每个 chunk scatter 到用户 iovec，不建立总长度 `gathered Vec`。
- 跨页 fault 不需要回滚文件 offset，但应返回正确部分进度。
- 不在 hot path 保留 `[sys_preadv]` 的 `info!`，诊断只能是 feature-gated trace 或累计
  计数。

## 公共执行器

建议抽取只表达 syscall 语义的内部 helper：

```rust
fn import_iovecs(...) -> Result<ImportedIoVecs, ErrNo>;
fn scatter_progress(...) -> UserWriteProgress;
fn read_transfer_len(requested: usize) -> usize;
```

helper 放 syscall impl 内，不把 ABI `ErrNo/UserIoVec` 下沉到 VFS/MM API。`read` 和
`readv` 共用 lease finish；`pread` 和 `preadv` 共用 positional staging。

本任务不顺手重构 write/writev/pwrite。可以记录它们存在的同类大分配问题，另立任务。

## 如何验收

最小矩阵：

- 单个 iovec >4 MiB 不返回 `EINVAL`，允许合法短读。
- iovcnt 1024 成功，1025 返回 `EINVAL`。
- iovec descriptor 指针溢出返回 `EFAULT/EINVAL` 且不消费 source。
- 第一 iovec 成功、第二 iovec fault：返回第一段字节数，source 只提交该前缀。
- 第一 iovec 首字节 fault：返回 `EFAULT`，source 不变。
- zero-length iovec 的 NULL base 合法。
- `readv(-1, NULL, 0)` 返回 `EBADF`。
- `pread` 不改变当前 offset。
- `pread(pipe)` 返回 `ESPIPE`。
- `pread(O_WRONLY file)` 返回 `EBADF`。
- 大 pread/preadv 不产生接近用户 count 的内核常驻 Vec。

执行：

```bash
cd os
make rv_check
make la_check
make kernel-rv-final
```

运行时用 iozone、lmbench、LTP readv/preadv 以及 RIO-10 定向程序验收。

## 搜索范围、并行与交付

用 `rg "sys_readv|sys_pread|UserIoVec|SYSCALL_IO_MAX|MAX_IO|IO_CHUNK"` 审核 syscall
dispatch、io.rs、fallible buffer 和所有 iovec helper。额外搜索 preadv2；若仍未实现，
只记录缺口，不在本任务无需求扩展新 syscall 号。

iovec 导入 helper 可在 RIO-05 至 RIO-08 期间并行开发，最终 readv 接入必须等待全部
source lease。测试放 syscall 模块和 guest 定向程序，日志放 `/tmp`。完成后在索引
勾选 RIO-09，记录上限、partial iovec、pread offset 和内存峰值。

## 禁止做法

- 不因内部 buffer cap 返回 `EINVAL`。
- 不在导入完整 iovec descriptor 前消费 source。
- 不把 readv 实现为多个独立阻塞 read。
- 不用 infallible `Vec::resize/extend` 构建用户可控的巨型 buffer。
- 不保留逐调用 `info!`/逐 iovec trace。
