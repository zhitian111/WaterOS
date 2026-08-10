# VFS another-ext4 路径查找缓存 FIFO 方案（2026-08-11）

## 为什么选择这里

当前 300 秒 pc-hot 中：

```text
AnotherExt4Fs::lookup      262,197,892
another_ext4::dir_find_entry 221,416,656
```

BuildStorm 会反复打开、stat、访问大量源码路径。`AnotherExt4Fs` 的 lookup cache
容量是 `4096`，但当前实现只要满就直接 `clear()`，导致每遇到第 4097 个新路径就把
全部已缓存路径清空，后续大量重复路径重新进入 ext4 目录项扫描和块缓存读取。

这是 VFS-01 中尚未实施的一个低风险小步：把“满后全清”改为有界 FIFO，保持
`BTreeMap` 查询复杂度和现有 create/unlink/rename 失效语义。

## 选择的方案

把 `lookup_cache: Mutex<BTreeMap<String, u32>>` 改为一个持有两部分的
`LookupCache`：

```rust
struct LookupCache {
    entries: BTreeMap<String, u32>,
    order: VecDeque<String>,
}
```

- 命中路径直接走 `BTreeMap`。
- miss 后查 ext4，成功后插入 `entries` 并把路径追加到 `order`。
- `entries.len() >= 4096` 时，从 `order` 前端弹出最旧路径并从 `entries` 删除；
  不再清空整表。
- `cache_remove_subtree` / `cache_rename_subtree` 同时维护 `entries` 和 `order`。
- FIFO 不移动命中项，避免在热路径把 BTreeMap 查找变成 O(n) LRU 更新。

## 为什么这么做

1. 改动局限在 another-ext4 adapter，不触碰 VFS/MM/scheduler/allocator。
2. 保持当前 BTreeMap 的 O(log n) 查询，不引入新的 hash 依赖。
3. 与 Linux dcache 的“保留最近使用路径”方向一致，但采用最小 FIFO，先验证收益；
   若证明仍有周期抖动，再升级为 clock/分段 LRU。
4. 失效路径仍精确按路径前缀处理，不会像整表 clear 一样丢失所有命中。

## 接下来的工作

1. 在 `perf/vfs-lookup-fifo` 分支实现 `LookupCache` 和三个维护函数。
2. 增加 cache 满后保留最近路径、subtree remove、rename 的单元测试。
3. 双架构 Final `make check`，180 秒 smoke。
4. RISC-V 完整 BuildStorm A/B；相对同轮 main 有 ≥ 1.5% 净改善才合并。
5. 若有效，补 pc-hot/wait-hot 前后对比并归档。

## 验收标准

- 双架构 Final check 通过。
- lookup cache 不再整表 clear，所有失效路径保持原语义。
- 完整 BuildStorm 无 panic/SIGSEGV，相对同宿主 main 有可复现收益。

## 实测结果（2026-08-11）

```text
vfs-lookup-fifo-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=907.81
main-5a080c07-full-b1:   BUILDSTORM_COMPILE ok=true elapsed_s=874.46
```

双架构 Final check 与 180 秒 smoke 均通过，但完整 BuildStorm 相对同轮 main 慢约
`33.35s`（+3.8%），明确退化。FIFO 维护队列的每次插入/删除额外成本高于避免整表
clear 的命中收益，因此实现已全部回退，仅保留本记录。
