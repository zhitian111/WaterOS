# 01 MachineDriver 外部中断与 per-CPU 初始化接口

## 任务内容

当前 `driver-api/api-v0` 的 `MachineDriver` 只有 `init_after_boot`、`realtime_ns`、
`test`，无法表达「外部中断到达时交给板级 driver」和「AP 启动时初始化当前 CPU」。
真实板（PLIC / LIOINTC / SATA 中断）需要这两个入口。

本任务扩展契约，并在 `trap_handler.rs` 接入外部中断路由；QEMU virt 提供等价实现，
保证两架构 QEMU 行为不变。

## 实施方案

1. 在 `driver-api/api-v0` 增加 `handle_external_interrupt(cause/irq: ...) -> DriverResult<()>`
   与 `init_current_cpu() -> DriverResult<()>`（具体签名以现有 trap/中断上下文为准）。
2. QEMU RV/LA 两个 `machine.rs` 实现新增方法（RV 走 PLIC 派发，LA 走 LS7A 外部中断或
   显式 no-op 以保持现状）。
3. `os/src/trap_handler.rs` 在外部中断分支调用 `driver::active_impl::machine().handle_external_interrupt(...)`。
4. `src/main.rs` AP 启动路径调用 `init_current_cpu()`（若有）。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-api/api-v0/src/lib.rs`
- `os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/machine.rs`
- `os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/machine.rs`
- `os/src/trap_handler.rs`、`os/src/main.rs`

CodeGraph：

```bash
codegraph explore "MachineDriver"
codegraph explore "handle_external_interrupt"
codegraph explore "trap_handler"
```

## 验收方式

- [ ] `MachineDriver` 新方法语义明确，默认实现不破坏既有 `machine()` 单例。
- [ ] 两 QEMU 外部中断路径调用链接通，QEMU 冒烟无 panic。
- [ ] 未引入阻塞/重入风险（中断上下文不持锁、不做用户拷贝）。

## 验收命令

```bash
cd os
make configure
make rv_check
make la_check
make rv_pre_run
make la_pre_run
git diff --check
```

## 验证环境

- L0 宿主机：`cargo check`。✅
- L1 QEMU virt：外部中断实际派发可在 QEMU virt 上跑（网络/VirtIO 中断）。✅
- L3 真机：仅契约层，真机验证后置到任务 06/11。❌

## 任务简报

（完成后追加，格式见目录 README。）
