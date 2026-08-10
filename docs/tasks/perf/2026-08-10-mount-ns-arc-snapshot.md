# mount namespace Arc 快照方案（2026-08-10）

## 为什么选择这里

pc-hot 显示：

- `mount_namespace_snapshot`：119,698,038 条指令
- `Vec<MountEntry>::clone`：80,827,347 条指令
- `Vec<MountEntry>::drop`：30,309,594 条指令

每次路径解析都会通过 `resolve_route -> mount_namespace_snapshot` 深拷贝整张辅助
挂载表，随后又释放该临时副本。挂载表在 BuildStorm 过程中基本只读，这种“读路径
每调用一次就深拷贝”的开销完全可以通过共享不可变快照消除。

## 选择的方案

把 `MountNamespace.entries` 从 `Vec<MountEntry>` 改为
`Arc<Vec<MountEntry>>`：

```rust
#[derive(Default, Clone)]
struct MountNamespace {
    entries : Arc<Vec<MountEntry>>,
}
```

- `mount_namespace_snapshot()` 不再逐项 clone，只增加一次 `Arc` 引用计数。
- 所有挂载表修改路径通过 `Arc::make_mut` 在写入时创建独立副本。
- bootstrap 挂载表改为 `OnceLock<Mutex<MountNamespace>>`，避免静态初始化时需要
  非 const 的 `Arc::new`。
- 读路径继续使用同一份 `&[MountEntry]`，不改变挂载语义。

## 为什么这么做

1. 挂载表在路径解析中是典型的“多读少写”，`Arc` 正好把读路径成本降到引用计数。
2. 与之前“持锁借用”方案不同，本方案不扩大 mount 表锁临界区，也不改变 route
   返回对象的所有权。
3. `Arc::make_mut` 只在真正修改时复制，正常路径解析不会复制任何 `MountEntry`。

## 接下来的工作

1. 在 `perf/mount-ns-arc-snapshot` 分支修改 `MountNamespace`。
2. 将 `entries` 的直接可变访问替换为 `Arc::make_mut`。
3. 将 `BOOTSTRAP_MOUNT_NS` 改为 `OnceLock`。
4. 双架构 `make check` 与 mount/procfs 相关定向测试。
5. 完整 RISC-V BuildStorm A/B；有效则合并 main，无效则回退并记录。

## 验收标准

- 双架构 Final check 通过。
- 路径解析、mount/unmount、proc/mounts、bind、rename 不回归。
- 完整 BuildStorm 相对 `880.44s` 有可复现改善。
