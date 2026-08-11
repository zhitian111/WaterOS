# K09 主办方新镜像 BuildStorm 夜间测试报告

## 环境

- 架构：RISC-V64，QEMU `virt`，8 hart，8 GiB RAM，OpenSBI。
- 根镜像：主办方修订后的 `os/sdcard-rv-pub.img`。
- 原始镜像 SHA-256：`61d1fb20a61d2af1bf2d1e7c8d0031eb0c867bb6599bd659b41465c7cf420926`。
- LoongArch 镜像 SHA-256：`cf8660bdc216d3dd6c82f4b50cdc4271d1be6dc49eb647ccbb9a0f24f36ad245`。
- 写盘方式：以原始镜像为只读 backing file 的 qcow2 覆盖盘。
- 内核：`kernel-rv-final-log`，包含 `stall-debug`。

## 已确认结果

1. another-ext4 能识别新镜像并以 RW 挂载根文件系统。
2. CAgent 10 项全部通过，耗时约 3.6 秒。
3. BuildStorm 工具链检查、mini build 和 `tg-xtask` 预构建通过。
4. 正式构建已越过此前发生 `SIGSEGV` 的 `compiler_builtins`，成功推进到最终 `arceos-helloworld` crate；未再出现 rustc `SIGSEGV`。
5. 中止后对覆盖盘执行 `e2fsck -fn`，Pass 1 至 Pass 5 全部通过；仅报告 extent tree 可选压缩，无 inode、目录、块位图或 extent 结构错误。
6. 主办方 LoongArch 原始镜像也通过 `e2fsck -fn` 全部五阶段检查，未发现结构错误；本轮未运行 LoongArch 全量 BuildStorm。

因此，当前 BuildStorm 阻塞不能归因于镜像损坏或本轮文件系统写坏。页缓存稳定节点写回修复已在真实编译负载下保持文件系统一致性。

## 尚未通过的问题

完整 BuildStorm 尚未自然结束，观察到两类非确定性停滞：

- 一轮中 CPU 4/6 的 timer 计数永久停止，对应运行任务和新建线程不再推进。
- 长时间轮次在最终 crate 后无日志和磁盘 I/O。GDB 采样显示 8 个 hart 全部位于 `__wateros_idle_task_runtime_main`，`sstatus.SIE=1`，最近 trap 均为 supervisor timer：CPU/时钟仍工作，但用户任务全部阻塞且没有任务重新入队。

长时间轮次还出现一次：

```text
[trap] killing user task (signal frame setup failed) ... task_id=270 parent_id=Some(266)
```

Cargo 随后继续编译，该告警不一定是最终停滞的直接原因，但必须保留为退出/信号通知链线索。

## 下一步诊断范围

优先检查以下路径，不再继续修改文件系统：

- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/`
- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/{clone,task,wait}.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/{futex,signal}.rs`
- pipe EOF、子进程退出、`ChildExit`/`TaskExit` waiter 的发布和唤醒路径。

下一次复现应增加独立于“系统调用总数”的周期快照，记录 runnable/blocked 数量、每个 wait queue 的成员、pipe 端点引用计数及最近一次任务状态迁移。验收标准仍是 BuildStorm 输出结束标记并以 0 退出，随后覆盖盘通过 `e2fsck -fn`。
