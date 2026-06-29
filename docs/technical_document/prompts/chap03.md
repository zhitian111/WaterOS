# 编写第 3 章：模块实现（整章）

## 任务目标

撰写或修订 **关键模块的实现说明**：沿用户程序运行路径，写清各子系统如何实现、如何衔接，并配代表性代码摘录。

## 输出模式（二选一，分发任务时须指明）

| 模式 | 输出 | `main.tex` |
|------|------|------------|
| **A. 整章**（当前默认） | `chapters/chap03/written-implementation.tex` | 保持 `\include{.../written-implementation}` |
| **B. 模块化** | `chapters/chap03/components/**` + `implementation.tex` | 改为 `\include{chapters/chap03/implementation}` |

本文件针对 **模式 A**。模式 B 见 [chap03-modular.md](chap03-modular.md)。

## 输出文件

- `docs/technical_document/wateros-latex/chapters/chap03/written-implementation.tex`

## 执行前必读

- `docs/technical_document/prompts/README.md`
- `docs/prompts/architecture.md`
- `docs/exports/architecture/module-relations.md`
- 与本章各节对应组件的 `docs/exports/features/*.md`、`docs/exports/public-api/*.md`

## 章节结构

```latex
\chapter{关键模块实现}
  % 章首 1 段：按用户态运行路径组织
\section{平台与运行时}
\section{异常、中断与系统调用入口}
\section{内存管理}
\section{任务管理与调度}
\section{设备驱动}
\section{文件系统与 VFS}
\section{系统调用与 ABI}
\section{IPC、凭证与内核日志}
```

## 各节写作要点

### 平台与运行时

- `wateros-platform`：双架构 boot context 差异（各一段 `rustcode`）
- `wateros-runtime`：console/logging/panic/heap 初始化顺序
- `wateros-klog` 与 runtime logging 的边界（一句即可，细节在最后一节）

### 异常、中断与系统调用入口

- `os/src/trap_handler.rs`：ecall → syscall、page fault → MM/signal、timer → schedule
- 说明 execve 成功时不推进 PC 等**非显而易见**语义

### 内存管理

- Sv39 vs LoongArch 页表 impl；`satp` / `PGDL` / TLB flush 摘录
- COW fork、`handle_user_page_fault`、`brk`/`mmap` 与 syscall 边界
- 可用子模块 `longtable` 列出 `kernel_global.rs`、`pagetable.rs` 等

### 任务管理与调度

- 任务状态、zombie、fork/clone 参数解码（LA vs RV）
- `schedule_tick` / `__switch` 摘录；与 IPC waitqueue 的衔接一句带过

### 设备驱动

- DTB VirtIO-MMIO vs PCI 枚举；统一到 `BlockDevice` 表
- 与 `fs::init` / devfs 刷新的数据流

### 文件系统与 VFS

- 启动数据流 enumerate：driver → devfs → fs probe → mount → vfs fd
- `openat`、`getdents64`、pipe、`PagedFileHandle` 等选 2～4 段关键 `rustcode`
- FS 层与 VFS 层职责分界

### 系统调用与 ABI

- `dispatch_syscall_from_trap`、`UserRet`、已接入 syscall 分类列表
- `wateros-abi` 编号表与 errno 约定

### IPC、凭证与内核日志

- futex wait 竞态检查、shm attach、`cred` fork 复制、`syslog` 与 klog 环

## 代码摘录规范

- 从**当前源码**复制，可删减无关行，用注释标 `...`
- 路径在正文或注释中标明（如 `os/components/wateros-mm/...`）
- 汇编与 Rust 布局对应处注明文件（如 `trap.S` ↔ `TrapContext`）

## 事实来源

- `os/components/**` 源码（含 impl 子 crate）
- `docs/exports/features/`、`docs/exports/public-api/`
- `docs/guides/` 中专题文（如 `ipc-futex-impl-task-design.md`、`filesystem-current.md`）

## 禁止

- 不重复第 2 章整段 feature 树与目录树（可一句引用）
- 不写 api-v0 全部 trait 逐条罗列（选代表接口）
- 不粘贴与节主题无关的大段测例日志

## 完成检查

- [ ] 每节至少 1 段叙述 + 1 个代码块或表
- [ ] 与第 2 章组件命名、启动顺序一致
- [ ] 双架构差异只在确有差异处对比，其余强调上层复用
- [ ] 文件顶部 `%` 说明注释保留

## 向模块化迁移时

将本节内容**剪切**到 `chapters/chap03/components/wateros-<name>/` 对应叶子 `.tex`，父级 `.tex` 写短综述 + `\input`；完成后改 `main.tex` 并跑 `annotate-tex-files.py`。
