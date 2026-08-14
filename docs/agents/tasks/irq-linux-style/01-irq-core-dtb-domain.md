# T01：IRQ 核心抽象 + DTB irq-domain 解析

## 任务内容

新增跨平台 IRQ 核心组件，并把现有 DTB 解析从「单个 `IrqLine`」升级为可解析
`interrupt-parent / interrupts / interrupt-map` 的 irq domain，得到统一 `virq`。
本任务不改变任何驱动行为，只落地基础设施与契约。

## 实施方案

1. 新增 `os/components/wateros-irq/`：
   - `IrqNumber`/`Virq` 类型；
   - `IrqChip` trait：`enable/disable/mask/unmask/ack/eoi/set_affinity`；
   - `IrqDomain`：`hwirq -> virq` 映射、父/子控制器与 `interrupt-map`；
   - `IrqAction` 注册表（handler + dev_id + bottom-half 标记，本任务先只注册不分发）。
2. 升级 `driver-api` 的 `IrqLine` 与 DTB 解析：
   - `os/components/wateros-driver/driver-api/api-v0/src/lib.rs`
   - `os/components/wateros-driver/driver-impl/impl-common/src/dtb.rs`
   - 保留现有 `parse_irq` 调用方兼容，新增完整 `interrupt-parent`/`interrupt-map` 解析。
3. 设计顶层 feature 传播：`wateros-irq` 由 `wateros-platform`/`wateros-driver` 依赖，避免反向依赖。

## 涉及文件

- `os/components/wateros-irq/**`（新增）
- `os/components/wateros-driver/driver-api/api-v0/src/lib.rs`
- `os/components/wateros-driver/driver-impl/impl-common/src/dtb.rs`
- 相关 `Cargo.toml`（feature/依赖传播）

## CodeGraph 查询命令

```bash
codegraph explore "IrqLine DeviceInfo parse_irq"
codegraph callers parse_irq
codegraph impact IrqLine
codegraph explore "compatible_list first_mmio_region is_virtio_mmio_compatible"
```

## 验收方式

- 静态：`make configure && make rv_check && make la_check`。
- 单测：`wateros-irq` 与 `driver-api`/`impl-common` 的 host `cargo test`（DTB irq 解析样例）。
- QEMU smoke：双架构启动不回归（本任务未启用设备 IRQ，行为应与 main 一致）。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make configure
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
# 功能 smoke 用官方 qemu_run / snapshot 入口即可
```

## 任务简报

完成后写 `history/01-irq-core-dtb-domain.md`。
