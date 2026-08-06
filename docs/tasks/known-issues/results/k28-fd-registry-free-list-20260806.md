# K-28 fd registry 空闲位图与增量计数（2026-08-06）

## 问题

`PerTaskFdRegistry` 每次 `alloc_fd` 从 0 线性扫描空闲槽，每次 open 前还通过遍历
整个 fd 表统计 open 数量。fd 表上限 1024 时，高频 open/close/dup 会退化为 O(N²)，
是 `docs/todo/perf-lock-resource.md` 中 L-6 的已知问题。

## 修改

- 为每个 owner 维护 `open_counts` 和 `free_fds` BTreeSet。
- `alloc_fd` 从 free set 取最低可用 fd，没有空闲时追加；open 计数增量维护。
- `close/close_range/dup3` 同步回收 fd 到 free set 并更新 open 计数。
- fork 复制 fd 表和线程共享 fd 表路径同步初始化 open/free 状态。

对外 API 不变，没有修改 task 模块或调度结构。

## 验证

```text
make rv_check
make la_check
make kernel-rv-final
make kernel-rv-pre
```

Final smoke：CAgent 10/10，VFS self-test 通过，无 panic 或 BadFd 回归。

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1575.50 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

Pre 可行性（`sdcard-rv.img`，60 秒）：进入 hackbench/cyclictest，无 panic 和 ext4
读块错误。

说明：完整 BuildStorm 耗时与本轮之前的 `1567.28s` 接近，属于噪声范围；该项修复关闭
了已知的 O(N²) 路径，但没有把总耗时降到 700-800s。
