
## 已知缺失功能汇总

### 已实现但未强制执行/简化的


| 功能                                | 文件       | 说明                                                             |
| ------------------------------------- | ------------ | ------------------------------------------------------------------ |
| `CLONE_PIDFD`                       | `clone.rs` | 接受标志，返回`ENOSYS`                                           |
| `set_tid` / `set_tid_size` (clone3) | `clone.rs` | 接受字段，返回`ENOSYS`                                           |
| `CLONE_CLEAR_SIGHAND`               | `clone.rs` | 标志被`validate_fork_clone_flags` 接受，但信号处理程序从未被清除 |
| `sched_setaffinity`                 | `sched.rs` | 权限检查通过，但亲和性掩码未存储，调度器从不检查                 |
| `PR_SET_NO_NEW_PRIVS`               | `task.rs`  | 接受并返回成功，但不强制（setuid 未实现）                        |
| `PR_SET_PDEATHSIG` 信号发送         | `task.rs`  | 实现了存储/查询，但父进程退出时未向子进程发送信号                |

### 已知缺失功能


| 功能                              | 影响                                                         |
| ----------------------------------- | -------------------------------------------------------------- |
| `prlimit64` 非零 PID              | 仅支持`pid == 0`（当前进程）                                 |
| `setrlimit` 权限检查              | 任何进程可任意设置自己的资源限制（无 CAP_SYS_RESOURCE 检查） |
| `PR_SET_MM`                       | checkpoint/restart 用，暂不需要                              |
| `PR_SET_PTRACER`                  | ptrace 未实现                                                |
| `PR_CAP_AMBIENT`                  | 能力模型未完整实现                                           |
| `sched_setattr` `sched_nice` 写入 | 接受字段但不处理（可通过`setpriority` 设置）                 |
| `cred-exec-setuid`                | 可执行文件 S_ISUID/S_ISGID 未更新凭证                        |
| vfork 父进程阻塞                  | vfork 退化为普通 fork                                        |
