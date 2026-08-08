# `madvise` VMA/映射范围检查（2026-08-08）

## 问题

`madvise02` 剩余失败有两类：

- `MADV_DONTNEED/FREE` 落在共享/文件映射上时未返回 `EINVAL`。
- `MADV_NORMAL/WILLNEED` 等 no-op advice 落在未映射区间上时未返回 `ENOMEM`。

## 修改

`wateros-mm` 新增两个地址空间接口，RISC-V 与 LoongArch 同步实现：

- `madvise_range_mapped`：逐页检查是否已有 PTE、lazy/file/shared VMA、栈或 brk。
- `madvise_range_shared_or_file`：检查范围是否与 lazy/file/shared 映射重叠。

`sys_madvise`：

- no-op advice 若范围未映射返回 `ENOMEM`。
- `MADV_DONTNEED/FREE` 若范围落在共享/文件映射返回 `EINVAL`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/madvise2-vma-fixed.log`：

```text
madvise02: 12 TPASS / 0 failed / 1 skipped
FAIL LTP CASE madvise02 : 0
```
