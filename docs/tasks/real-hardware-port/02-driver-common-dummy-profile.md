# 02 通用驱动胶水与 dummy profile

> 历史说明：本任务记录早期 bring-up 期间使用的 dummy 过渡方案。合入 `main` 时该
> machine driver 已清理；当前必须显式选择 QEMU RISC-V、QEMU LoongArch64、JH7110
> 或 Loongson 2K1000LA 平台，未选择平台会直接报配置错误。

## 任务内容

从 `feat/real-hardware-common` 移植 `driver-impl/impl-common`（DTB 辅助 `dtb.rs` +
VirtIO DMA 公共路径 `virtio_dma.rs`），并新增 `impl-dummy` driver profile 作为真实硬件
profile 的占位与模板，让「无真实外设」阶段仍可编译、可单测。

目标是把 DTB 解析和 VirtIO DMA 这两块跨板共享的逻辑从板级实现里抽出来，避免 JH7110 和
2K1000 各自重复。

## 实施方案

1. 迁移 `impl-common/src/{lib,dtb,virtio_dma}.rs`，审计错误语义与 `no_std`。
2. 新增 `driver-impl/impl-dummy`：实现 `MachineDriver`（含任务 01 新方法）的空/最小版本。
3. `wateros-driver/src/lib.rs` 增加 `impl-dummy` 的 feature 选择与再导出。
4. 为 `dtb.rs`、`virtio_dma.rs` 补 host 单测（用固定 DTB fixture / 地址对齐用例）。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-impl/impl-common/**`（新增）
- `os/components/wateros-driver/driver-impl/impl-dummy/**`（新增）
- `os/components/wateros-driver/src/lib.rs`
- `os/components/wateros-driver/Cargo.toml`

CodeGraph：

```bash
codegraph explore "MachineDriver"
codegraph explore "virtio_dma"
codegraph explore "active_impl"
```

## 验收方式

- [ ] `impl-common` 的 DTB 解析纯函数有 host 单测且通过。
      （`virtio_dma` 因 main 帧分配器无连续页 API 且会破坏 host 可测性，顺延任务 10）
- [ ] `impl-dummy` 能通过 `--features ...impl-dummy` 编译。
- [ ] 默认 QEMU feature 组合不受影响。

## 验收命令

```bash
cd os
make configure
make rv_check
make la_check
cargo test -p wateros-driver-impl-common   # 以实际 package 名为准
git diff --check
```

## 验证环境

- L0 宿主机：host 单测 + `cargo check`。✅
- L1 QEMU virt：VirtIO DMA 路径在 QEMU virt 下被 VirtIO 设备覆盖。✅
- L3 真机：本任务不涉及。❌

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-common/dtb.rs` 已存在于 main（与旧分支逐字一致），本任务为其纯函数
    （`read_be_u32` / `is_virtio_mmio_compatible`）补 host 单测（2 个通过）。
  - 新增 `driver-impl/impl-dummy`：干净的 `MachineDriver` 最小实现（移除旧分支的
    “AI 完成”注释与 `add()` 占位函数），外部中断/每 CPU 初始化走 trait 默认语义。
  - `wateros-driver`：workspace 成员 + 非可选依赖；`machine()` 缺省回退 dummy
    （替代原来的 `core::unreachable!`）。
- 计划调整（基于当前 main 证据）：
  - `virtio_dma.rs` **顺延任务 10**：main 帧分配器没有旧分支的
    `frame_alloc_contiguous`/`FrameSpan` API；引入帧分配器依赖会让 impl-common
    失去 host 可测性，且当前 QEMU virtio HAL 是各自内联 DMA，共享模块暂为
    未被消费的平行实现。任务 10（AHCI DMA provider）时按 main 的
    `frame_alloc_result`/`frame_dealloc_result` API 一并实现。
- 验收结果：
  - `cargo test -p wateros-driver-impl-common`：2 passed（host）。
  - `cargo check -p wateros-driver-impl-dummy`：通过。
  - `cargo check -p wateros-driver`（qemu RV/LA 两种 feature，各自 target）：通过。
  - `make rv_check`、`make la_check`：通过（仅既有 warnings）。
  - `git diff --check`：clean。
- 未验证/风险：
  - dummy 作为无 arch feature 构建的兜底，受 `wateros-platform-arch` 既有依赖链
    约束，真正可构建的接线留待任务 04。
