# RISC-V scheduler 负载均衡导致 current-task 与硬件栈错配

## 现象

完整 RISC-V Final BuildStorm 在加入“空闲偷取 + 唤醒亲和性放宽”后出现：

```text
[trap] restore_current_trap_frame failed before sret to user
current task kind=Kernel state=Running returns_to_user=true
recursive heap allocation detected
```

临时诊断还稳定观察到：

```text
[trap] frame outside current task kernel stack
frame=0x822e5178 bottom=0x805f1b30 top=0x805f9b30
```

即 CPU 实际运行用户任务并使用该任务的内核栈时，scheduler 仍认为当前任务是
idle/kernel 任务。这个错配会进一步污染 allocator 的 per-CPU guard 深度，表现为
`CfsQueue::pick` / `scan_epoll_ready` 等路径的 `recursive heap allocation`。

## 根因

`pick_next_runnable_or_steal()` 在空闲核上从其它 CPU 偷取任务，并把偷来的任务直接
选为本地 next task。该路径在 `__switch` 前更新 CPU current-task 缓存的语义与
协作式切换不一致，导致硬件上下文和 scheduler current-task 脱节。

## 修改

`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/`：

- `scheduler.rs` 恢复从本地 runqueue 选择 next task，不再调用空闲偷取。
- 移除 `pick_next_runnable_or_steal`、`steal_ready_task`。
- 保留唤醒亲和性放宽 `cpu_is_overloaded`，它不参与 next-task 选择。

## 验证

- `make check ARCH=rv PROFILE=final` 通过。
- `make check ARCH=la PROFILE=final` 通过。
- RISC-V Final 完整 BuildStorm 通过：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1146.02 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
all commands finished
```

原始日志：

```text
/tmp/final-rv-ra18.log
/tmp/final-rv-ra19.log
```
