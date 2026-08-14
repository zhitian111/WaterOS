# 02 通用驱动胶水与 dummy profile

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

- [ ] `impl-common` 的 DTB 解析与 VirtIO DMA 有 host 单测且通过。
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

（完成后追加，格式见目录 README。）
