# wateros-pseudo-shell — 已实现功能快照

## 用途

记录阻塞式 UART REPL 的 bring-up 验证能力。事实来源：`os/components/wateros-pseudo-shell/src/lib.rs`。

## 组件形态

| 项 | 说明 |
|----|------|
| 结构 | 单 crate，无 api/impl 子目录 |
| 依赖 | `runtime-serial`、`vfs`、`task`、`mm`（RISC-V exec）、`cred` |
| 启用 | 根 `os/Cargo.toml` 可选 feature `pseudo_shell` |

## Feature

| Feature | 效果 |
|---------|------|
| `default` | `cred/impl-root`（exec 后 cred/cwd/fd 清理） |

## 已实现命令

| 命令 | 行为 |
|------|------|
| `help` / `?` | 打印命令列表 |
| `cd` | 相对/绝对路径切换内存 cwd |
| `ls` | `root::read_view().read_dir` |
| `stat` | 打印节点类型、size、mode |
| `rm` | ext4 RW session `unlink` |
| `exec` | **仅 riscv64**：ELF 装载、spawn 用户任务、wait/reap |

## 调用约定

须在 `driver::init_after_boot`（UART 就绪）、`task::init` 与调度器运行后，从**内核任务**调用 `run_pseudo_shell()`（不返回）。

## 缺口

- LoongArch 等平台 `exec` 打印 unsupported。
- 无管道、重定向、脚本；stdin 非行编辑。
- 故意不依赖 `wateros-runtime` 聚合，避免与 `mm` 环依赖。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
