# wateros-platform — 发布概览

## 用途

面向集成与 bring-up 的简短说明：本组件在整仓中的位置、默认 feature 选择与验证方式。细节见 `features/`、`public-api/`、`architecture/` 同名校验文档。

## 组件定位

`wateros-platform` 是内核访问 **硬件与运行环境** 的统一入口，把三层拼在一起：

1. **架构层**（`platform-arch`）：与 ISA 绑定的 trap、CSR、分页原语。
2. **板级层**（`platform-impl`）：QEMU/OpenSBI 或 LoongArch virt 的 console、timer、reset、引导参数。
3. **聚合层**（根 crate）：`timer` 等组合 API，以及 `wall_clock` 等跨层语义。

内核其它组件应优先依赖 `platform::` 再导出，而不是直接引用 `arch-impl-*` 或 `platform-impl-*`（除非编写新 profile）。

## 当前支持的 profile

| Profile | Feature | 目标 |
|---------|---------|------|
| LoongArch QEMU virt | `impl-qemu-loongarch64-virt` | 主线 bring-up（默认成员 crate 常开） |
| RISC-V QEMU + OpenSBI | `impl-qemu-riscv64-opensbi` | RISC-V 自检与 busybox 路径 |
| 占位 | `impl-dummy` + 默认 arch | 仅编译占位，不可启动 |

## 依赖方要点

- **定时器**：先 `platform::time::set_frequency_hz`（若 DTB 已探测），再 `platform::timer::set_timer_after*`；arch 只读 tick，platform-impl 写 deadline。
- **Trap**：`platform::arch::init()` 后，组合层必须 `register_kernel_trap_handler`。
- **分页**：`platform::arch::paging` 与 `wateros-mm` 配合；token 来源为 MM 的 `kernel_satp` / 用户地址空间。
- **Panic/日志**：`wateros-runtime` 的 shutdown 路径调用 `platform::reset::shutdown`。

## 验证

```bash
# LoongArch 聚合检查
cd os/components/wateros-platform
cargo check --features impl-qemu-loongarch64-virt

# RISC-V 聚合检查
cargo check --features impl-qemu-riscv64-opensbi
```

完整内核启动验证见根 `os/` 对应 target 的 QEMU 脚本与 LTP/自检日志。

## 已知限制（当前快照）

- 双架构 feature 不可同时启用。
- 频率默认值（RISC-V 10 MHz、LoongArch 100 MHz）在 DTB 未覆盖时为回退，非所有真机通用。
- FPU 完整上下文切换与部分信号路径仍为演进中能力。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
