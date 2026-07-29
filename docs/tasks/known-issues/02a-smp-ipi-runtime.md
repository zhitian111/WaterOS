# K-02A：双架构 8 核与 IPI 运行验证

## 任务目标

独立验证 RISC-V64/OpenSBI 与 LoongArch64/IOCSR 的 8 核启动、远端调度通知和 TLB
shootdown。只修当前可复现的 SMP 根因，不重复实现已经存在的平台接口。

## 执行前必读

- `docs/tasks/known-issues/02-smp-loongarch-validation.md`
- `docs/prompts/general.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-platform.md`
- `docs/exports/features/wateros-task.md`
- `docs/单核设计转多核设计需要更改的位置的预记录.md`

## 已知信息与代码证据

平台聚合层已用 per-CPU 原子位保存 IPI 原因，再发送硬件通知：

```rust
PENDING_IPI[cpu].fetch_or(kind.bits(), Ordering::Release);
crate::active_impl::smp::SmpImpl::send_ipi(mask)
```

RISC-V 已有 SBI HSM/IPI，LoongArch 已有 IOCSR send/clear。开放项是双架构运行证据和
竞态验证，不是接口缺失。

## 涉及文件

- `os/src/{main,trap_handler}.rs`
- `os/components/wateros-platform/{platform-api/api-v0/src/smp.rs,src/lib.rs}`
- `os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/lib.rs`
- `os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/lib.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/`
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/`

## 任务内容

1. 两架构以 `-smp 8 -m 8G` 启动，记录 configured/online mask 和每核 timer/task。
2. 循环远端 enqueue、wake、notify 和 reschedule，核对 pending reason、IPI clear 与
   scheduler 检查顺序。
3. 并发 `mmap/munmap/fork/exec`，验证 remote TLB flush 覆盖所有运行该地址空间的
   CPU。
4. 检查 SBI/IOCSR 调用、user-copy 和大对象 drop 都不发生在 scheduler/MM 自旋锁内。
5. 仅在复现失败后修改对应 `api-v0` 或平台 impl；平台寄存器细节不能进入 task。

## 如何验收

- [ ] 两架构均报告 8 CPU online，`cpuinfo`、affinity、`nproc` 一致。
- [ ] 每核运行过用户 task、timer 和 idle；10,000 次远端 wake 无丢失。
- [ ] IPI 合并/清除无 storm，runnable task 不会长期留在 idle CPU 外。
- [ ] TLB 压测无旧映射、错页、UAF 和跨进程泄漏。
- [ ] `make rv_check && make la_check` 通过，结果回填 K-02 报告。

交付 `docs/tasks/known-issues/results/k02a-YYYYMMDD.md`。
