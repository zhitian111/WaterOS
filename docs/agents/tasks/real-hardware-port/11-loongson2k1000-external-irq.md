# 11 Loongson 2K1000 外部中断（LIOINTC）最小接入

## 任务内容

在 2K1000 上接入外部中断控制器（优先 LIOINTC），让后续 SATA 中断/DMA、GMAC 等有可用的
外部 IRQ 路径。第一阶段 SATA 是 polled，本任务非首轮阻塞项，但为任务 10 之后的功能铺路。

优先评估现成轮子 `irq-loongarch`（no_std，覆盖 EIOINTC/PCH-PIC/LIOINTC）能否替代/收窄
旧分支 22K 行手写 INTC 代码；保持 MIT。

## 实施方案

1. 调研 `irq-loongarch` 的 nightly/映射模型/许可证，决定「引入」还是「按 DT binding 手写
   最小 LIOINTC」。
2. 接入任务 01 的 `handle_external_interrupt`：读 CPU 中断 cause → LIOINTC/EIOINTC 派发 →
   清中断 → 分发到设备。
3. 旧分支的 `liointc.rs`/`board_irq_owner.rs`/`irq_runtime.rs` 只作寄存器对照，不整体导入。
4. 定时器/中断闭环先在真机验证：定时器中断能反复进入并退出。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-impl/impl-loongson2k1000la/**`
- `os/src/trap_handler.rs`

CodeGraph：

```bash
codegraph explore "handle_external_interrupt"
codegraph explore "trap_handler"
codegraph explore "irq"
```

## 验收方式

- [ ] `--features loongson2k1000la,pre` 能编译。
- [ ] 定时器中断在真机稳定反复触发（真机项）。
- [ ] 外部 IRQ 派发/清中断路径有单测或最小闭环验证。
- [ ] 未整体导入旧分支 22K 行 INTC 代码。

## 验收命令

```bash
cd os
make configure
make la_check
git diff --check
```

## 验证环境

- L0 宿主机：`cargo check`、逻辑单测。✅
- L3 真机：LIOINTC/EIOINTC 实际时序与分发。🔴（必须）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - **轮子评估结论**：引入 `irq-loongarch` 0.1.1-pre.1（MIT/Apache-2.0，no_std，
    `Liointc` 提供 init/enable/disable/claim/complete）替代手写 LIOINTC；核对
    `loongArch64` crate 的 ESTAT 位布局：HWI0=bit2，与 WaterOS 的 IPI（bit12）/
    Timer（bit11）不冲突。
  - LA trap 解码：`decode_loongarch64_trap_cause` 在 IPI/Timer 之后、异常之前增加
    `ESTAT.IS.HWI0..HWI7`（bit2..=9）→ `Interrupt::SupervisiorExternel` 分支
    （QEMU LA 无外部中断，休眠路径，无回归）。
  - 2K1000 驱动新增 `liointc.rs`：
    - `ClaimComplete` trait 抽象 claim/complete（host 单测闭环）；
    - 真实实现包住 `irq-loongarch::Liointc`（LoongArch 目标）；
    - `init_current_cpu`（BSP 上 `init()` 一次，路由 core0/HWI0，开 ECFG LIE）；
    - `handle_external_interrupt`：claim → 无设备 handler 记日志 → complete；
    - 基址用 Linux 2K1000 DTS 常量 `0x1fe0_1400`（main）/`0x1fe0_1540`（isr0），
      标注真机确认 TODO。
  - 接线：`MachineDriver::handle_external_interrupt` / `init_current_cpu` 接入；
    `irq-loongarch` 依赖 target-gated（host 不编译，liointc 单测可跑）。
  - 根 README 第三方依赖新增 `irq-loongarch`。
- 验收结果：
  - `cargo test -p wateros-driver-impl-loongson2k1000la`：3 passed（host：
    claim→派发→complete 闭环、空 claim、越界拒绝）。
  - `cargo check --no-default-features --features loongson2k1000la,pre
    --target loongarch64-unknown-none`：通过（`loongArch64` 0.2.6 编译正常）。
  - `make la_check`、`make rv_check`：无回归；`git diff --check`：clean。
- 未验证/风险：
  - 真机 LIOINTC 时序与分发未验证（定时器/外部中断闭环需 2K1000 板）。
  - LIOINTC 基址为 Linux DTS 常量，未经真机确认；无 DTB 时不做 DTB 发现
    （后续可接 `loongson,liointc-2.0` 节点解析）。
  - 第一阶段无设备 handler 表；SATA 保持 polled，外部中断为后续 DMA/网络铺路。
