# BS-SC-10：RISC-V dirty-only FPU 保存

## 任务内容

依据 `sstatus.FS` 仅在用户 FPU 状态 Dirty 时保存 32 个 FPR/fcsr。该任务是高风险架构状态
改造，必须独立验收；不能只在现有汇编前加条件跳转，因为当前整个栈上 `TrapContext` 会按值
复制到 TCB，跳过 `fsd` 会把未初始化字节覆盖到已保存状态。

## 实施方案

1. 先画出 Off/Initial/Clean/Dirty 状态机以及首次用户进入、普通 trap、signal、fork、exec、
   context switch、跨核迁移的转换。
2. 将持久化 `UserFpuState` 与每次都复制的 GPR/CSR trap frame 明确分离，或让 frame 同步 API
   支持“只更新非 FPU 部分”；不得读取未初始化栈存储。
3. trap 入口：FS=Dirty 才保存并发布新 FPU state；FS=Clean 复用已保存状态；Off/Initial
   按 ABI 初始化。
4. 返回用户态：已装载状态设为 Clean，不再由 `set_return_to_user()` 无条件标为 Dirty。
5. signal frame 必须看到最新 FPU 状态，rt_sigreturn 恢复后状态机一致。
6. fork 复制父状态，exec 初始化，迁移不得引用旧 CPU 的 per-CPU scratch。
7. 本任务不实现完全 lazy per-CPU FPU owner；若 dirty-only 无法在现有布局下安全完成，提交
   拒绝简报，不强行扩大范围。

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/{asm/trap.asm,src/trap.rs}`
- `os/components/wateros-task/task-impl/impl-core/src/tcb.rs`
- task scheduler trap-frame registry/runtime 路径
- signal machine context 编解码路径
- `os/scripts/perf/guest-probes/riscv_fpu_state.sh`
- `history/10-brief.md`

## CodeGraph 查询

```bash
codegraph explore "RISC-V TrapContext FPU sstatus FS set_return_to_user SignalFrameCodec"
codegraph impact "TrapContext"
codegraph explore "fork trap frame exec signal context migration"
codegraph callers "set_return_to_user"
```

## 验收方式

RISC-V probe 使用不同 bit pattern 覆盖全部 32 FPR，覆盖 syscall、timer、两线程抢占、CPU
affinity 迁移、signal handler 使用 FPU、fork 和 exec，至少 100,000 次切换。还要运行无 FPU
程序，证明不会触发意外非法指令。

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check
make kernel-rv-final
make la_check
riscv64-linux-gnu-objdump -d kernel-rv-final > /tmp/bs-sc-10-rv.dump
git diff --check
```

完整 RISC-V BuildStorm 按 `<500/500..520/>=520` 规则验收；LoongArch 只需 check/build smoke，
因为本提交不得改变其代码。

## 完成后简报

新增 `history/10-brief.md`，附 FS 状态转换表、FPU probe 计数、反汇编保存/恢复分支、RISC-V
性能结果及未覆盖硬件差异。
