# `madvise` 未实现 advice 返回 `EINVAL`（2026-08-08）

## 问题

LTP `madvise02` 对当前未实现的 `MADV_REMOVE`、`MADV_MERGEABLE`、
`MADV_UNMERGEABLE`、`MADV_WIPEONFORK` 等 advice 期望 `EINVAL`，内核此前直接
当作 no-op 返回成功。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs`：

- 仍保持 `MADV_NORMAL/RANDOM/SEQUENTIAL/WILLNEED/HUGEPAGE/COLD` 等 no-op 成功。
- `MADV_REMOVE`、`MADV_MERGEABLE`、`MADV_UNMERGEABLE`、`MADV_WIPEONFORK`、
  `MADV_KEEPONFORK`、`MADV_COLLAPSE` 返回 `EINVAL`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/madvise-unsupported-fixed.log`：

```text
madvise02 失败数从 10 降为 5
MADV_REMOVE / MERGEABLE / UNMERGEABLE / WIPEONFORK x3 均 TPASS
```

同一轮还复验了 `lstat02/02_64`，当前全部通过。

## 后续

剩余失败集中在 `MADV_DONTNEED/FREE` 对共享/文件映射的约束，以及未映射区间上的
`ENOMEM` 判断，需要补齐 VMA 类型查询后继续。
