# Task 07：建立大型 crate 的开始/完成时间与内核链路画像

## 任务目标

验证 `uefi`、`unwind`、`hashbrown`、`ax-posix-api` 等长尾 crate 的真实墙钟占比，区分
rustc 用户态 CPU、内核 syscall、I/O 等待、页错误和调度停顿，避免对单核链路盲目优化。

## 实施方案

1. 保留现有 workload 和 QEMU 参数；在 host runner 记录 cargo JSON/human `Compiling` 行的
   接收单调时间。利用 `compiler-artifact` 完成事件配对，输出每个 crate 的 start/end/duration。
2. 若 runner 无法可靠配对，增加 feature-gated 内核进程 exec/exit 摘要：命令名、用户/内核
   tick、阻塞 tick、syscall 类计数和 fault 计数；默认构建完全关闭。
3. 使用 QEMU 9.2.1 的 ecall/pc-hot 诊断窗口，对最长的 3 个 crate 分段采样；诊断不通过
   串口逐事件打印，不纳入最终性能成绩。
4. 将每个长尾窗口分类为：用户态 CPU、scheduler/futex、mmap/mprotect/page fault、
   VFS metadata、block/virtio I/O。

## 验收方式

```bash
python3 --version
cd os
make rv_check
make la_check
```

使用任务 00 的镜像各跑一次 300 秒诊断和一次完整 smoke；结果必须能列出至少 5 个 crate
的闭合时间区间，且不改变 TOOLCHAIN/MINIBUILD/BUILDSTORM 成功率。诊断 kernel 不得成为
性能候选；必须与无诊断 kernel 分离保存。

## 涉及文件与 CodeGraph

- `os/scripts/perf/` 或本分支新增 host 诊断工具
- `os/src/user_bringup_common.rs`
- `os/components/wateros-task/**`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/bringup_stats.rs`

```bash
codegraph explore "user_bringup_common exec exit runtime task statistics syscall counters"
codegraph callers "log_thread_bringup_stats_summary"
```

## 完成后

新增 `history/07-brief.md`，附 crate duration 表、分类占比、诊断开销和选定的一个链路。

