# 任务 15：mmap 返回实际 PTE 变化摘要

## 任务内容与目标

让新建 lazy anonymous/private file VMA 在没有修改 PTE 时跳过本地全 TLB flush 和远端
shootdown；eager shared/device、MAP_FIXED 覆盖等仍按实际变化同步。本提交只处理 mmap。

## 实施方案

1. 在 MM 契约引入 `PteChange`（至少 `None/Changed`；若平台已支持可扩展 Range/Full）。
2. mmap 各实现返回地址和变化摘要；错误路径保留保守 full flush，因为可能部分改页表。
3. syscall 使用 conditional helper；lazy VMA-only 成功返回不 flush。
4. 双架构对称修改，统一注明同步责任归 facade，删除 mmap 内部重复 fence。
5. 测试 lazy none、eager changed、MAP_FIXED resident/nonresident 和失败中途修改。

## 涉及文件

- `os/components/wateros-mm/mm-api/api-v0/src/mmap.rs`
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/src/user_heap_mmap.rs`
- 双架构 `user_aspace.rs` conditional helper
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs`

## CodeGraph 查询

```bash
codegraph explore "MmapOps::mmap with_user_aspace_mut_and_flush mmap_file_lazy mmap_device"
codegraph impact "MmapOps"
codegraph callers "with_user_aspace_mut_and_flush"
```

## 验收方式

```bash
cd os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
# mmap/MAP_FIXED/shared/device/fault 定向测试
cd .. && git diff --check
```

任务 01 计数证明 lazy mmap no-change 分支不产生 flush/shootdown；resident/MAP_FIXED 回归无 stale
TLB。任务 00 runner完成 RISC-V A/B，并记录 mmap 调用与跳过比例。

## Commit 与简报

提交建议：`[perf] mmap 按实际 PTE 变化同步 TLB`。新增 `history/15-brief.md`。
