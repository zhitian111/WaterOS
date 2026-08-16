# BS-SC-01：去除 RISC-V 用户返回重复 FPU restore

## 任务内容

当前 `trap_entry_rust` 返回后无条件恢复 32 个 FPR/fcsr，用户返回随后又在
`__wateros_riscv_restore_user_from_frame` 恢复同一状态。本提交按保存的 `sstatus.SPP`
区分内核/用户返回，用户路径只保留后一套恢复。

## 实施方案

1. 在通用返回点读取栈上 `sstatus` 的 SPP；返回用户态时跳过第一套 `fld`/`fcsr`。
2. 返回内核态继续使用现有恢复，首次用户任务进入和 fork/exec 仍通过
   `__wateros_riscv_restore_user_from_frame` 完整恢复。
3. 不改变 `TrapContext` 布局、FPU 保存策略、FS 状态或 LoongArch 代码。
4. 反汇编确认用户返回控制流只有一套 32 FPR restore，内核返回仍有一套。

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm`
- 必要时同目录 `src/trap.rs` 的布局/控制位常量注释
- `history/01-brief.md`

## CodeGraph 查询

```bash
codegraph explore "__alltraps __wateros_riscv_restore_user_from_frame TrapContext FPU"
codegraph impact "TrapContext"
codegraph callers "trap_entry_rust"
```

## 验收方式

本提交属于低风险批次 A，不单独跑完整 BuildStorm。

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check
make kernel-rv-final
riscv64-linux-gnu-objdump -d kernel-rv-final > /tmp/bs-sc-01-rv.dump
rg -n "trap_entry_rust|restore_user_from_frame|fld|fcsr|sret" /tmp/bs-sc-01-rv.dump
cd ..
git diff --check
```

功能门槛：启动/首次用户进入、普通 syscall、timer 抢占和 BuildStorm minibuild 均无非法指令、
寄存器污染或 trap storm。完整双架构与性能验收由任务 04 执行。

## 完成后简报

新增 `history/01-brief.md`，记录反汇编中两条返回分支的 restore 数量、最窄检查结果，并标记
“批次 A 完整验收待任务 04”。
