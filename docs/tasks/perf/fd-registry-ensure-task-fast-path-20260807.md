# fd registry `ensure_task` 快速路径

## 优化思路

短负载 pc-hot 显示 `PerTaskFdRegistry::ensure_task` 是内核侧 Top-5 热点。该函数在
每次 fd 操作都会执行多次 BTreeMap `entry`/`contains_key`：

```rust
fn ensure_task(&mut self, task_id) {
    if !self.owners.contains_key(&task_id) { ... }
    let owner = self.effective_owner(task_id);
    self.ref_counts.entry(owner).or_insert(1);
    let table = self.tables.entry(owner).or_insert_with(Vec::new);
    ...
}
```

绝大多数 syscall 发生在已经初始化过 stdio 的普通任务中，不需要再次修改 owner、
refcount 或 fd 表长度。新增快速返回路径：

```rust
if let Some(&owner) = self.owners.get(&task_id) {
    if let Some(table) = self.tables.get(&owner) {
        if table.len() >= VFS_FIRST_DYNAMIC_FD &&
           self.ref_counts.contains_key(&owner)
        {
            return;
        }
    }
}
```

这样已初始化任务只需几次只读 BTreeMap 查询，不再进入可写的 `entry`/insert 路径。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`

## 验证

- `make check ARCH=rv PROFILE=pre`
- `make check ARCH=la PROFILE=pre`
- `make check ARCH=rv PROFILE=final`
- `make check ARCH=la PROFILE=final`
- RISC-V pre QEMU 60s smoke：rootfs RW 挂载成功，进入 busybox bringup，无 panic。
- RISC-V QEMU 90s pc-hot 同负载运行无 panic，`ensure_task` 仍为主要热点之一，
  说明 fd registry 本身还有进一步优化空间。

## 后续

继续观察 BuildStorm 全量采样中的 `ensure_task` / `io_handle_for_task`。如果仍高，
下一轮考虑减少 `SharedIoHandle` 的 Arc/Mutex 层次，或在 syscall 热路径缓存 task
的 owner/fd 表指针。
