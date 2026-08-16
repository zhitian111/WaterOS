# BS-SC-11：LoongArch LSX/FPU 按所有权切换

## 执行门禁

仅当任务 07/09 后 LoongArch 仍未低于最终目标，且 pc-hot/反汇编证明 trap 中 32 个
`vst`/`vld` 是显著热点时执行。否则提交一份“无需执行”简报，不改架构状态机。

## 任务内容

当前每次 LoongArch trap 保存和恢复 32 个 128-bit LSX 寄存器、FCSR 和 FCC，`TrapContext`
为 832 字节。LoongArch 没有可直接等价于 RISC-V FS Dirty 的现成路径，本任务采用明确的
per-CPU vector owner/lazy enable 设计，只在所有权转移或 signal/fork 需要 materialize 时保存。

## 实施方案

1. 先验证 QEMU 9.2.1 对 `EUEN.FPE/SXE` 禁用后的首次 FPU/LSX 异常编码和恢复行为；该探针
   不通过则停止实现。
2. 每 CPU 记录当前 LSX owner task id/generation；内核不得使用 FPU/LSX。
3. 同一用户 task syscall 往返且未调度切换时保留寄存器 resident，不执行全量 save/restore。
4. 真正切换到另一 task、signal frame 捕获、fork、exec、exit 或迁移时 materialize/释放 owner。
5. task id generation 防止回收复用；跨核迁移先在旧 CPU materialize，再发布到新 CPU。
6. kernel trap 不触碰 resident 用户 LSX；嵌套 trap 的 owner 状态必须可证明。
7. 同步 `TrapContext`、signal machine context 和 switch/trap 汇编注释及大小断言。

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/{asm/trap.S,asm/switch.S,src/trap.rs}`
- LoongArch CPU/task context 与 task lifecycle 路径
- signal frame 编解码
- `os/scripts/perf/guest-probes/loongarch_lsx_state.sh`
- `history/11-brief.md`

## CodeGraph 查询

```bash
codegraph explore "LoongArch TrapContext LSX EUEN FPE SXE SignalFrameCodec"
codegraph impact "TrapContext"
codegraph explore "context switch migration fork exec signal LoongArch"
```

补充汇编检索：

```bash
rg -n "SAVE_LSX|RESTORE_LSX|EUEN|vst|vld|fcsr|fcc" \
  os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64
```

## 验收方式

LSX probe 为全部 32 个向量寄存器写不同 128-bit pattern，覆盖 syscall、timer、双线程、12 CPU
迁移、signal handler、fork/exec，至少 100,000 次 owner 变化。任何一次污染、异常循环或
signal frame 不一致都拒绝提交。

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make la_check
make kernel-la-final
make rv_check
loongarch64-unknown-linux-gnu-objdump -d kernel-la-final > /tmp/bs-sc-11-la.dump
git diff --check
```

完整 LoongArch BuildStorm 按总规则验收；RISC-V 做 check/build smoke。

## 完成后简报

新增 `history/11-brief.md`，记录执行门禁证据、异常编码、owner 状态机、LSX probe、反汇编和
LoongArch 性能结果；未执行时明确记录为何无需承担该风险。
