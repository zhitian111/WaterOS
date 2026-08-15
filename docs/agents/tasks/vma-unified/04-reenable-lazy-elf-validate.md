# Task 04：重新打开 `elf-lazy-map` 并做双架构完整回归

## 任务目标

在 VMA 有序性修复后，恢复 Sv39 默认开启 `elf-lazy-map`，证明 lazy 路径不再是
RV SIGSEGV 根因；同时做 RV/LA 完整功能回归。

## 实施方案

1. 恢复 `mm-impl/impl-sv39/Cargo.toml` 默认 feature：

   ```toml
   default = [ "api-v0", "elf-lazy-map" ]
   ```

2. 先跑 RV/LA 单核完整 BuildStorm；
3. 再跑线上参数：
   - LA `-smp 12`
   - RV `-smp 8`
4. 记录每轮结果和日志 SHA。

## 涉及文件

- `os/components/wateros-mm/mm-impl/impl-sv39/Cargo.toml`
- `os/components/wateros-mm/mm-impl/common/src/vma/**`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/kernel_elf.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-vma-unified
codegraph explore "map_segment_from_path_lazy handle_lazy_page_fault"
codegraph impact "map_segment_from_path_lazy"
```

## 验收方式

```bash
cd /tmp/wateros-vma-unified/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
git diff --check
```

运行时：

- RV 单核、RV 8 核、LA 单核、LA 12 核完整 BuildStorm；
- 无 `SIGSEGV signal not delivered`、无 panic/OOM；
- `BUILDSTORM_RESULT status=OK run=OK`。

## 完成后

新增 `history/04-brief.md`，记录性能变化。
