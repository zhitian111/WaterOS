# wateros-vfs 功能快照

## 当前状态

- **`vfs-api-v0`**：VFS 侧错误、路径规范化、`SingleRootReadView` / `RootRwSession` 契约（不依赖 **`wateros-fs`**）。
- **`vfs-impl-dummy`**：无后端占位，供默认 feature 下独立编译。
- **`vfs-impl-fs-bridge`**（feature **`bridge-fs-api`**）：通过 **`wateros-fs`** 公开 API 提供单根只读委托、RW 挂载会话、能力查询与 devfs 枚举；与多挂载 inode 树无关，属 bring-up 桥接层。

## 根 crate 接线

- **`wateros`** 默认 **`qemu-riscv64-opensbi`** 启用 **`vfs-bridge`**，从而在驱动与 **`fs::init` / `fs::test`** 成功后调用 **`vfs::test()`** 及 **`vfs::bridge::rw_write_root_verify_via_ro`**（与 fs 自检可重复写同一根文件，语义与 fs 聚合测试 RW 段一致）。

## 工作区说明

- **`wateros-vfs`/`Cargo.toml` 的 `[workspace].members`** 仅包含 **`vfs-api/api-v0`** 与 **`vfs-impl/impl-dummy`**，避免在宿主目标上对 **`wateros-vfs-impl-fs-bridge`** 执行 `cargo check` 时误拉 **`wateros-fs`** 的 RISC-V 专用依赖链；桥接 crate 仍作为路径依赖在 **`riscv64gc-unknown-none-elf`** 等目标下由 **`wateros`** 正常编译。

## 后续关注点

- 多挂载、vnode、与 task/fd 打通时扩展 **`vfs-api`**，并保持 **`wateros-fs`** 作为具体 FS impl 的后端。
- 能力或导出变化时同步 [`docs/exports/public-api/wateros-vfs.md`](../public-api/wateros-vfs.md) 与本文件。
