# BS-SC-01 任务简报

## 状态

接受，批次 A 完整功能与性能验收待 BS-SC-04。本提交只改变 RISC-V trap 返回汇编，commit
为 `1c2d5322f79f52203980922618222e463cac8462`。

## 修改与影响面

- 修改 `impl-riscv64/asm/trap.asm`：在 FPU restore 前读取保存的 `sstatus.SPP`；
- SPP=0 的用户返回直接进入 `__wateros_riscv_restore_user_from_frame`；
- SPP=1 的内核返回保留原有 32 FPR 和 `fcsr` 恢复；
- 未改变 `TrapContext` 布局、保存策略、FS 位、用户 trampoline 或 LoongArch 路径。

CodeGraph 查询 `trap_entry_rust`、`__alltraps`、
`__wateros_riscv_restore_user_from_frame` 和 `TrapContext` impact。`TrapContext` 影响面为 91 个
符号，但本次未修改结构体或 API；汇编 helper 的非 Rust 调用边由反汇编补充确认。

## 实际验收

```text
make rv_check                         PASS（仅有既有 warning）
make kernel-rv-final                  PASS（release，7.36s）
riscv64-linux-gnu-objdump -d ...      PASS
git diff --check                      PASS
```

反汇编地址 `gdb_point_1=0x80201238`：`bnez t6,0x80201254` 位于所有 `fld` 前；未跳转时在
`0x80201250` 直达 `__wateros_riscv_restore_user_from_frame=0x80201302`。内核返回块为 32 条
`fld`，用户 helper 也为 32 条；用户路径不经过前者，内核路径不经过后者。两处各保留一次
`fcsr` 恢复和一次最终 `sret`。

## 决定与剩余风险

静态控制流和 RISC-V 构建证明低风险改动符合设计，保留该提交。本任务没有单独启动 guest；
首次用户进入、syscall、timer 抢占、minibuild 和 BuildStorm 的运行时验证统一由 BS-SC-04
关闭。文档只新增历史简报，当前架构/API/构建接口未变化，无其它同步项。
