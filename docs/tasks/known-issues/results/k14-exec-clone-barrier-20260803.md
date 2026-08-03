# 多线程 exec/clone 屏障修复记录

## 问题

多线程进程执行 `execve` 时，旧实现先获取线程列表，再逐个终止 sibling。并发
`clone(CLONE_THREAD)` 可能在线程列表快照之后注册新线程，使旧地址空间仍被执行，或在
`retain_only_task_in_process` 后留下调度器与进程注册表状态不一致。

## 修复

- 在进程注册表锁内原子设置 `exec_in_progress` 并取得稳定线程列表。
- 屏障生效期间拒绝新 member 注册；exec 保留当前线程后解除屏障。
- 清理循环等待远端运行线程响应重调度，并分别处理已退出待回收和已不存在的任务。
- 增加注册表单元测试，覆盖屏障期间 clone 被拒绝、完成 exec 后 clone 恢复。

## 验收

- `make check`：通过。
- `make la_check`：通过。
- 该屏障的前一版参与了 8 核 BuildStorm 完整通过验证；本次收口把列表快照并入同一临界区。
- host `cargo test` 受现有 `wateros-platform-arch` dummy paging 类型未导出问题阻塞，未能执行；
  测试代码已随实现保留。
