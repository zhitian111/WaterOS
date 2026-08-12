# task syscall 实现备忘

[项目首页](../../../../../../../../README.md) · [内核工程](../../../../../../../README.md) · [wateros-syscall](../../../../../README.md)

| 优先级 | 项目                            | 价值                                                                                                                                       |
| -------- | --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| P0     | 调度 syscall 权限检查           | `sched_setparam`、`sched_setscheduler`、`sched_setattr`目前不像`sched_setaffinity`一样检查目标进程权限；普通进程可能改其他任务的调度属性。 |
| P0     | `nice`真正参与`SCHED_OTHER`调度 | 当前`setpriority`只写进程字段，源码明确“不参与调度”。因此调用成功却没有优先级效果。                                                      |
| P0     | 信号中断与 syscall restart      | `wait*`、`futex`、`nanosleep`等被信号打断时，应正确返回`EINTR`或按`SA_RESTART`重启。这也和你刚才的 LTP timeout/卡住排查直接相关。          |
| P0     | `PR_SET_PDEATHSIG`实际投递      | 当前只保存信号号；父进程退出时需向仍存活子进程投递该信号。                                                                                 |
| P1     | `sched_rr_get_interval(2)`      | 当前支持 RR 却没有给用户态查询时间片的接口。实现可直接返回调度器 RR quantum 对应的`timespec`。                                             |
| P1     | `getcpu(2)`                     | SMP 下很值得补：让 libc 的`sched_getcpu()`获得当前 CPU。只需将当前`CpuId`拷贝给用户指针；node/cache 参数可先返回 0。                       |
| P1     | `getsid(2)`                     | 已有`setsid`、`setpgid`、`getpgid`，缺少配套查询。shell/job control 与测试更完整。                                                         |
| P1     | `prlimit64(pid != 0)`           | 现在只支持当前进程。应支持同 uid 的目标进程、特权进程操作任意目标；并补资源限制实际生效。                                                  |
| P1     | 真正执行 rlimit                 | 目前`RLIMIT_NOFILE`、`NPROC`、`AS`、`STACK`等只是记录值。至少应让`NOFILE`限制 open/dup，`NPROC`限制 fork/clone，`AS`限制 mmap/brk。        |
| P2     | `CLONE_CLEAR_SIGHAND`           | 当前接受标志但不会清空子进程的 signal disposition，和 Linux 不一致。                                                                       |
| P2     | 真正的`vfork`                   | 现在退化为普通 fork，功能可用但语义/性能不对；实现需要让父任务阻塞到子进程`execve`或`exit`。                                               |
| P2     | pidfd 全套                      | `CLONE_PIDFD`、`pidfd_open`、`pidfd_send_signal`、`waitid(P_PIDFD)`要一起做，适合进程监管场景，但工程量较大。                              |
| 暂缓   | `rseq(2)`                       | 多线程 libc 可能探测它，但完整实现要求调度切换时维护用户 rseq ABI；先返回`ENOSYS`通常可接受。                                              |
| 暂缓   | `setns`/ 更多 namespace         | 现有`unshare(CLONE_NEWNS)`还比较基础，先完善 mount namespace 生命周期更合适。                                                              |
