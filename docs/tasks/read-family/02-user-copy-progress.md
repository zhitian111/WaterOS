# RIO-02：保留用户拷贝的部分进度

## 任务目标

为双架构用户写入提供“复制了多少字节、随后发生什么错误”的稳定契约，使读取源能只
提交已经到达用户空间的前缀。保持现有 `copy_to_user()` 调用方兼容，不在本任务引入
用户页 pin。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-mm.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/public-api/wateros-mm.md`
- `docs/exports/public-api/wateros-syscall.md`
- `docs/tasks/read-family/README.md`

## 已知信息与代码证据

`UserMemoryOps::copy_to_user()` 当前返回 `MmResult<usize>`。两个架构的实现都逐页复制，
但中途错误通过 `?` 直接返回，已经完成的 `done` 丢失：

```rust
while done < kernel_src.len() {
    let perm = aspace.leaf_page_perm(vpn)?;
    // ...
    dst.copy_from_slice(&kernel_src[done..done + chunk]);
    done += chunk;
}
Ok(done)
```

syscall 包装又把任何错误压成单一 errno：

```rust
ops.copy_to_user(VirtAddr(ptr), buf).map_err(mm_err_to_errno)
```

跨页缓冲区第一页可写、第二页无映射时，第一页可能已经被修改，但调用方只看到
`EFAULT`，无法决定底层数据应提交多少。

## 涉及文件

- `os/components/wateros-mm/mm-api/api-v0/src/user_access.rs`
- `os/components/wateros-mm/mm-api/api-v0/src/error.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/user_access.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/user_access.rs`
- `os/components/wateros-mm/src/lib.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/user_copy.rs`

## 建议契约

在 `api-v0` 增加不依赖 syscall errno 的结构。允许等价命名：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserCopyProgress {
    pub copied: usize,
    pub error: Option<MmError>,
}

pub trait UserMemoryOps {
    fn copy_to_user_progress(&self, dst: VirtAddr, src: &[u8]) -> UserCopyProgress;
}
```

约束：

- `error == None` 时 `copied == src.len()`。
- `error != None` 时 `copied <= src.len()`，并精确表示错误前已写入的前缀。
- 地址加法溢出必须在写下一段前报告，不能 wrap。
- 缺页和 COW 仍在当前地址空间锁内处理。
- 零长度操作返回 `{ copied: 0, error: None }`，不访问地址。

现有接口保持兼容：

```rust
fn copy_to_user(&self, dst: VirtAddr, src: &[u8]) -> MmResult<usize> {
    let progress = self.copy_to_user_progress(dst, src);
    match progress.error {
        None => Ok(progress.copied),
        Some(error) => Err(error),
    }
}
```

syscall 层增加对应包装，保留 MM 错误到 errno 的映射：

```rust
pub(crate) struct UserWriteProgress {
    pub copied: usize,
    pub error: Option<ErrNo>,
}
```

不要一次性改写全部 36 个 `copy_to_user` 调用方。本任务只提供新能力和测试；读取调用族
由 RIO-04、RIO-09 接入。

## 为什么不做 pin

帧分配器已有 `frame_inc_ref/frame_dealloc_result`，但引用计数目前同时承载 fork/COW
映射引用。若先 pin 一个可写物理页，随后另一线程 fork 将该页设为 COW，再由内核直接
写 pinned PA，会绕过 COW。完整 pin 需要额外 pin 类型、fork 复制策略、munmap 延迟
回收和地址空间销毁协议，超出本任务范围。

读取提交协议可以在不 pin 的情况下根据实际 copy 进度提交/回滚，因此禁止用裸 PA
列表替代本任务。

## 任务内容

- RISC-V 与 LoongArch 必须共用相同 API 语义。
- 每页先完成 fault/COW、权限检查和 PA 翻译，再执行该页 copy。
- 捕获每个可能失败的调用，返回当时的 `done`；不要在循环内部继续使用会丢进度的 `?`。
- 错误日志仍只在 syscall 诊断层按需输出，MM 热路径不逐页打印。
- 不把 `OutOfMemory` 无条件映射成 `EFAULT`；继续复用 `mm_err_to_errno`。

## 并行与边界

本任务可与 RIO-01、RIO-03 并行。API 提交应先于两个架构实现提交。后续读取源只依赖
聚合 `wateros-mm` 导出的稳定类型，不直接依赖 `impl-sv39` 或
`impl-loongarch64`。

## 如何验收

至少增加双架构等价测试：

1. 单页完整复制：`copied == len`、无错误。
2. 跨两个有效页完整复制。
3. 第一页尾部有效、第二页无映射：`copied` 等于第一页剩余长度，错误为访问错误。
4. 第一页只读且不能 COW：`copied == 0`。
5. 地址末尾加法溢出：不 wrap、不写越界。
6. COW 页写入成功且不修改共享旧帧。
7. 旧 `copy_to_user()` 对失败调用仍返回原有 `MmError`。

执行：

```bash
cd os
make rv_check
make la_check
```

若 host 单元测试因目标 feature 无法运行，必须通过内核组件自测或 RIO-10 的 guest
跨页测试补足，不能只报告“编译通过”。

## 搜索范围、并行与交付

用 `rg "copy_to_user|UserMemoryOps|MmError"` 审核 `wateros-mm` 聚合、API、两个 arch
impl 和 syscall wrapper。不要修改与用户写入无关的 loader/ELF 调用方。

本任务可与 RIO-01、RIO-03 并行。代码和组件测试放原组件目录，运行日志放 `/tmp`。
完成后在 `docs/tasks/read-family/README.md` 勾选 RIO-02，记录双架构测试和旧 API
兼容结果。
