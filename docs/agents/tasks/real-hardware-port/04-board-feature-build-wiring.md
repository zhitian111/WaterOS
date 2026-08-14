# 04 双板 feature 与构建接线

## 任务内容

新增两个板级 feature：`jh7110-visionfive2`（RISC-V）与 `loongson2k1000la`（LoongArch），
沿「顶层 Cargo.toml → platform/driver 聚合 crate → 实现 crate」传播，先以空/最小平台桩
保证 `--no-default-features --features <board>,<stage>` 能解析并通过 `cargo check`。

本任务只搭构建骨架，不实现真实板级逻辑（后续 05/09 填充）。

## 实施方案

1. 顶层 `os/Cargo.toml` 增加两个 feature 与平台 profile 选择。
2. `wateros-platform`、`wateros-driver` 聚合层增加对应 feature 与 `active_impl` 再导出。
3. 新增最小平台桩（可复用任务 02 的 `impl-dummy`，或用空 `lib.rs`），避免未实现符号。
4. `os/Makefile` / `scripts/configure.bash` 增加两板的目标与 check 入口。
5. 保持 default（QEMU RISC-V）不变，互斥 feature 约束仍成立。

## 涉及文件 / CodeGraph 查询

- `os/Cargo.toml`、`os/Makefile`、`os/scripts/configure.bash`
- `os/components/wateros-platform/Cargo.toml`、`src/lib.rs`
- `os/components/wateros-driver/Cargo.toml`、`src/lib.rs`

CodeGraph：

```bash
codegraph explore "active_impl"
codegraph explore "machine"
```

## 验收方式

- [ ] 两板 feature 组合 `cargo check` 通过（RV 与 LA 各自）。
- [ ] 默认 feature 与既有 QEMU 目标不受影响（`make check` 仍绿）。
- [ ] feature 互斥约束（RV/LA 平台 profile 不共存）成立。

## 验收命令

```bash
cd os
make configure
make rv_check
make la_check
cargo check --no-default-features --features jh7110-visionfive2,pre
cargo check --no-default-features --features loongson2k1000la,pre --target loongarch64-unknown-none
git diff --check
```

## 验证环境

- L0 宿主机：`cargo check` 两架构 feature。✅
- L1 QEMU virt：默认 QEMU 回归。✅
- L3 真机：不涉及（仅骨架）。❌

## 任务简报

（完成后追加，格式见目录 README。）
