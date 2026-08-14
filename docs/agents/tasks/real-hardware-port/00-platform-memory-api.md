# 00 平台内存布局 API

## 任务内容

为平台层补一个统一的内存布局契约：`KernelMemoryLayout`、`PhysicalRange`、
`MemoryLayoutError`（以及 `kernel_layout()` / 相关探针接口）。当前 main 的
`wateros-platform` 只有 `physical_ram_end_exclusive()`，且是 QEMU 分支硬切的逻辑，
没有表达「物理 RAM 区间 + MMIO 区间 + 探测策略」的统一入口。

物理板必须从 DTB 推导 RAM/MMIO/probe 布局，本任务先立契约并让 QEMU 两架构迁移到该契约，
行为与现状一致（无回归），后续板级实现（任务 05/09）再填充各自的布局。

## 实施方案

1. 从 `feat/real-hardware-common` 移植 `platform-api/api-v0/src/memory.rs`（审计后适配）。
2. 在 `wateros-platform/src/lib.rs` 增加 `active_impl::memory::kernel_layout()` 门面，
   替换/收窄现有 `physical_ram_end_exclusive()` 的 QEMU 硬切逻辑。
3. `impl-qemu-riscv64-opensbi`、`impl-qemu-loongarch64-virt` 分别实现其布局（保持与
   现有 DTB/固定值一致）。
4. 不改动 `api-v0` 之外的上层语义；纯契约 + 门面 + 两 QEMU 实现迁移。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-platform/platform-api/api-v0/src/memory.rs`（新增）
- `os/components/wateros-platform/src/lib.rs`
- `os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/**`
- `os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/**`
- 对应 `Cargo.toml`

CodeGraph：

```bash
codegraph explore "physical_ram_end_exclusive"
codegraph explore "KernelMemoryLayout"
codegraph explore "device_tree_phys_addr"
```

## 验收方式

- [ ] `memory.rs` 契约定义清晰：区间端点、空区间、错误语义（溢出/无解）明确。
- [ ] 两 QEMU 实现输出与迁移前的 RAM/MMIO 边界一致（无行为回归）。
- [ ] `wateros-platform` 不再有 QEMU 分支硬切物理 RAM 的散落逻辑。

## 验收命令

```bash
cd os
make configure
make rv_check
make la_check
git diff --check
```

## 验证环境

- L0 宿主机：`cargo check` 两架构、`git diff --check`。✅
- L1 QEMU virt：`make rv_pre_run` / `make la_pre_run` 冒烟，确认启动内存不变。✅
- L3 真机：本任务不涉及。❌

## 任务简报

（完成后追加，格式见目录 README。）
