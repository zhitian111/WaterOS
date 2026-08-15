# Task 00 简报：VMA 现状盘点

## 结论

Sv39 与 LoongArch64 的 VMA 实现高度重复，`lazy_file_vmas` 依赖“有序且无重叠”的
隐式不变量，但所有 split/merge/protect/remove/mremap 路径都在两套文件中手工重建
`Vec`。该不变量一旦被破坏，二分查找会漏页并导致用户进程 SIGSEGV。

## 四类 VMA

| 类型 | Sv39 | LoongArch64 |
|---|---|---|
| `LazyFileVma` | `impl-sv39/src/pagetable.rs:248` | `impl-loongarch64/src/pagetable.rs:250` |
| `SharedAnonVma` | `impl-sv39/src/pagetable.rs:279` | `impl-loongarch64/src/pagetable.rs:281` |
| `SharedFileVma` | `impl-sv39/src/pagetable.rs:284` | `impl-loongarch64/src/pagetable.rs:286` |
| `DeviceVma` | `impl-sv39/src/pagetable.rs:292` | `impl-loongarch64/src/pagetable.rs:294` |

## 重复修改路径

| 函数 | Sv39 | LoongArch64 |
|---|---|---|
| `lazy_vma_overlaps` | 468 | 432 |
| `merge_lazy_file_vma_perm` | 504 | 470 |
| `register_lazy_file_vma` | 844 | 816 |
| `remove_lazy_file_vmas` | 869 | 869 |
| `protect_lazy_file_vmas` | 906 | 906 |
| `lazy_file_vma_index` | 1232 | 1155 |

两套文件中对 `lazy_file_vmas` 及相关 VMA 字段的引用共约 285 次，说明后续抽公共层
不能只改类型，还要同步所有调用点。

## 有序性依赖

- `lazy_vma_overlaps` 用 `partition_point(|vma| vma.end <= start)`；
- `lazy_file_vma_index` 用相同方式按 `end` 二分；
- `register_lazy_file_vma` 用 `partition_point(|vma| vma.start < start)` 插入；
- `protect_lazy_file_vmas` 先按 `end`/`start` 定位，再 `insert(last, right)`、
  `insert(first, left)`，插入顺序与索引变化最复杂，是首要审计对象。

## 可疑失序入口

1. `protect_lazy_file_vmas`：分裂后先插入 right，再插入 left，若索引计算错误会破坏顺序；
2. `merge_lazy_file_vma_perm`：跨多个 VMA 时逐段 push left/mid/right，依赖原列表有序；
3. `remove_lazy_file_vmas`：裁剪后 push left/right，依赖 drain 顺序；
4. `mremap`：删除旧区间、注册新区间，依赖调用顺序正确；
5. `fork_cow` 使用 `iter().map(duplicate)` 保留顺序，但只复制不校验。

## 下一步

Task 01 先把 VMA 类型和基础方法抽到 `mm-impl/common`；Task 02 再用统一注册表
强制有序、无重叠，避免两个架构继续各写一套 Vec 维护逻辑。
