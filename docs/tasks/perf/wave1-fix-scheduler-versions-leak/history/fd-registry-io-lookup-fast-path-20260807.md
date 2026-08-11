# fd registry I/O 查找合并快速路径

## 优化思路

上一轮给 `ensure_task()` 增加了已初始化快速返回，但 `io_handle_for_task()` 和
`duplicate_handle_for_task()` 仍会在快速路径外再调用一次 `effective_owner()`，
造成重复的 BTreeMap owner 查询。

本轮把“已初始化 owner”判断抽成：

```rust
fn initialized_owner(&self, task_id) -> Option<TaskId> {
    let owner = self.owners.get(&task_id).copied()?;
    let table = self.tables.get(&owner)?;
    (table.len() >= VFS_FIRST_DYNAMIC_FD &&
     self.ref_counts.contains_key(&owner)).then_some(owner)
}
```

`ensure_task()`、`io_handle_for_task()`、`duplicate_handle_for_task()` 三处共用。
后两者在常见路径直接拿 owner 后立即访问 fd 表，不再进入 `ensure_task()` +
`effective_owner()` 的重复查询。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`

## 验证

- `make check ARCH=rv PROFILE=pre`
- `make check ARCH=la PROFILE=pre`
- `make check ARCH=rv PROFILE=final`
- `make check ARCH=la PROFILE=final`
- RISC-V pre QEMU 60s smoke：rootfs RW 挂载成功，进入 busybox bringup，无 panic。

## 后续

短负载热点中 fd registry 和 TLSF 仍占较高比例。下一步优先观察完整 Final 中
`io_handle_for_task` / `SharedIoHandle::with_io` 的占比，再决定是否减少 fd slot
的 Arc/Mutex 层次。
