# 任务 15 简报：mmap 条件同步 PTE

## 实现

- private anonymous mmap 和 lazy file mmap 在非 `MAP_FIXED` 成功路径报告无叶 PTE 变化。
- eager shared、device 和 `MAP_FIXED` 路径保持保守全量 TLB 同步。
- 操作失败时 `with_user_aspace_mut_and_flush_if_changed` 仍执行全量同步。

## 验证

- `make rv_check`
- `make la_check`
- `make kernel-rv-final`
- `make kernel-la-final`
- `git diff --check`

尚未执行 QEMU mmap workload A/B 和 flush/shootdown 计数验证。
