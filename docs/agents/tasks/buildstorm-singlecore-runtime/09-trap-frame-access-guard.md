# BS-SC-09：复用权威 trap frame 访问句柄

## 任务内容

当前用户 trap 入口获取 scheduler 锁，把栈上 `TrapContext` 复制到 TCB；出口再次获取全局
scheduler 锁，把 TCB frame 复制回栈。本任务保留 TCB 权威模型和两次复制，只消除第二次
scheduler 查找/锁，为后续更深改造建立稳定所有权。

## 实施方案

1. `begin_current_trap_frame_access` 返回稳定 guard/token，而不是只返回裸权威指针。
2. guard 持有足以保证 TCB 存活的引用和 task id；不能持有 scheduler/global registry 锁跨越
   syscall、block 或 context switch。
3. exit 使用同一 guard 将权威 frame 写回原内核栈，不再次调用
   `restore_current_trap_frame()` 查找 current task。
4. syscall 阻塞后恢复、跨核迁移、exec 替换 frame、signal/rt_sigreturn、退出不返回路径分别
   明确 guard 是否仍有效；必要时提供 replace/invalidate 方法。
5. 首次用户任务进入仍使用现有 scheduler restore 路径，不强行复用不存在的 trap guard。
6. 本提交不消除 560/832 字节复制，不改变 arch `TrapContext` 布局。

## 涉及文件

- `os/components/wateros-task/src/{runtime,trap}.rs`
- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/registry.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/{lib.rs,scheduler.rs}`
- `os/components/wateros-task/task-impl/impl-core/src/tcb.rs`
- `os/src/trap_handler.rs`
- `history/09-brief.md`

## CodeGraph 查询

```bash
codegraph explore "begin_current_trap_frame_access restore_current_trap_frame TrapFrameAccess"
codegraph callers "restore_current_trap_frame"
codegraph impact "begin_trap_frame_access"
codegraph explore "block_current schedule migration execve_current rt_sigreturn trap frame"
```

## 验收方式

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
```

guest stress 必须混合 futex/nanosleep/read 阻塞、线程迁移、fork/exec、signal handler 与
rt_sigreturn，至少运行 30 分钟或 100,000 次 trap 返回；不得出现 frame 属主错配。随后按总
README 规则跑双架构 BuildStorm。诊断/反汇编证明普通用户返回只取得一次 scheduler 锁。

## 完成后简报

新增 `history/09-brief.md`，写清 guard 生命周期证明、所有提前返回点、压力结果、锁次数和
双架构性能结果。
