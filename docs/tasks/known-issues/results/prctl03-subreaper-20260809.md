# `prctl03` child subreaper 语义修复（2026-08-09）

## 问题

LTP `prctl03` 依赖 `PR_SET/GET_CHILD_SUBREAPER` 和退出时的子进程托孤语义，
当前实现存在三处偏差：

1. syscall 常量把 SET/GET 反了：内核把 `PR_SET_CHILD_SUBREAPER=36` 处理成
   GET，把 `PR_GET_CHILD_SUBREAPER=37` 处理成 SET。
2. `PR_GET_CHILD_SUBREAPER` 直接把 0/1 作为 syscall 返回值，而 Linux 要求把
   结果写入 `(int *) arg2`，syscall 返回 0。
3. 进程退出时所有孤儿统一托孤给 init，没有查找最近的 living subreaper；
   fork 时又错误地继承了父进程的 subreaper 标记。

## 修改

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs`：
  - 修正 `PR_SET_CHILD_SUBREAPER=36`、`PR_GET_CHILD_SUBREAPER=37`。
  - GET 改为把 `0/1` 写入用户地址，成功返回 0，空指针返回 `EFAULT`。
- `os/components/wateros-task/task-impl/impl-core/src/process.rs`：
  - fork/clone 新进程不再继承 `child_subreaper`（exec 保留，符合 Linux）。
  - `reparent_orphans` 从父进程的父进程开始向上查找最近的 living subreaper；
    没有则回退到 PID 1。
  - 新增 `fork_clears_child_subreaper` 和
    `reparents_to_nearest_living_subreaper` 单元测试。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V 静态最小回归程序（不依赖 LTP harness）输出：

```text
GRANDCHILD_PPID=3 MAIN_PID=3
SUBREAPER_OK pid=5
```

这验证了：

- SET/GET 常量正确；
- GET 能写回用户地址；
- 中间父进程退出后，孤儿孙进程被托孤给 subreaper；
- subreaper 能通过 `wait()` 回收被托孤进程。

完整 LTP `prctl03` 在当前临时拼装的 glibc 镜像里仍在 LTP harness 初始化后以
`SIGSEGV` 结束，未进入 `PR_SET_CHILD_SUBREAPER` 的语义断言；静态回归已覆盖
本次内核语义修复，完整 LTP 需要后续用标准 pre 镜像重新验证后再从排除名单移除。
