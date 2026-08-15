# Task 03 简报：统一 lazy VMA 的 split/merge/protect/remove/mremap 操作

## 完成情况

Task 02 的 `LazyVmaSet` 已经封装了 split/merge/protect/remove，因此本任务
主要是确认所有调用点不再直接重建 `lazy_file_vmas`：

- `merge_lazy_file_vma_perm` → `LazyVmaSet::merge_perm`
- `protect_lazy_file_vmas` → `LazyVmaSet::protect_range`
- `remove_lazy_file_vmas` → `LazyVmaSet::remove_range`
- `register_lazy_file_vma` → 排序插入 + `sort`
- `mremap` → 继续使用统一的 `remove_lazy_file_vmas` +
  `register_lazy_file_vma`
- `handle_lazy_page_fault` → `LazyVmaSet::lookup` + `get/get_mut`

两套架构不再各自实现这些区间算法。

## 验收

- Task 02 已执行 `make rv_check` / `make la_check` / 双架构 final build，均通过；
- 本任务没有新增源码改动，只补记统一状态。

## 未验证项

- QEMU 运行时回归延后到 Task 04。
