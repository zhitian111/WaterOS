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

（完成后追加，格式见目录 README。）
