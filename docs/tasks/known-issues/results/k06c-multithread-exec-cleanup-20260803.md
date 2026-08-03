# K-06C 多线程 exec 资源清理修复结果（2026-08-03）

## 结论

修复多线程进程执行 `execve` 时遗留 sibling futex 等待登记和 epoll 状态的问题。该
泄漏会随 `rustup`、`rustc` 等工具反复启动而累积，是 BuildStorm 后期线程等待异常的
一个确定前置问题。修改位于 syscall 资源层，没有改变 task API、调度器或进程状态机。

## 根因与修改

`terminate_other_threads_for_exec()` 已正确完成 task/process registry 的 kill、reap 和
retain，但 `execve` 随后的资源循环只释放 cwd、mount namespace、fd、Unix socket、
credential 和 shm，遗漏：

- `ipc::futex::cancel_task_wait()`；
- epoll per-task 状态；
- 统一资源清理函数后续新增的其他侧表。

现在 `execve` 对每个被移除的 sibling 调用既有
`drop_task_runtime_resources_with_aspace()`，并把该函数限制为 syscall crate 内可见。
robust list 仍在旧地址空间销毁前处理，signal 状态仍由 `on_exec()` 统一收尾。

涉及文件：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/execve.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/wait.rs`

## 验证

- `make check`：通过。
- `make la_check`：通过。
- RISC-V64/OpenSBI、8 CPU、8 GiB，决赛 glibc 镜像：两次相同定向测试均完成
  12,800 次线程创建、barrier 和 join，分别约 41 秒。
- 修复前：首次 guest `rustc` 返回后保留 7 个 futex 队列，登记任务 38–44 已不在
  scheduler 活动任务中。
- 修复后：同一时点仅有 2 个 futex 队列，登记任务均可在 scheduler 快照中找到，分别
  为正在运行和已唤醒但尚未返回 syscall 的当前测试线程；不存在已消失任务登记。
- 临时 guest 源码、脚本、ELF 和内核诊断字段均已删除，正式启动命令已恢复。
- 测试后 `e2fsck -fn sdcard-rv-pub.img` 无结构损坏，仅有 extent tree 可优化提示；
  inode/block 使用量恢复到测试前数值。

## 后续

该修复证明并消除了 exec 侧表泄漏，但尚不能单独证明此前 BuildStorm 最终 join 停滞
完全解决。白天未运行完整 final；下一夜间窗口需要用干净主办方镜像重新执行完整
CAgent 和 BuildStorm，并检查 futex 队列是否长期有界。
