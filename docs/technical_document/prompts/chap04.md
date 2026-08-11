# 编写第 4 章：测试、复现与问题处理

## 任务目标

撰写或修订测试、构建、双架构验证与问题排查说明，使读者能复现内核运行并定位常见故障。

## 输出文件

- `docs/technical_document/wateros-latex/chapters/chap04.tex`

## 执行前必读

- `docs/technical_document/prompts/README.md`
- `docs/prompts/general.md`（构建与运行）
- `docs/prompts/coding.md` §6（若存在 Makefile 约定）
- `docs/prompts/tasks/run_testsuits_qemu.md`
- `docs/prompts/tasks/analyze_kernel_log.md`
- `os/Makefile`、根目录 `Makefile`

## 章节结构

```latex
\chapter{测试、复现与问题处理}
\section{构建与启动方式}
\section{功能验证}
\section{双架构一致性验证}
\section{遇到的问题和解决方法}
  \subsection{...}  % 按主题分，见下
```

## 各节写作要点

### 构建与启动方式

- `bashcode` 列出常用命令：`make all`、`make kernel-rv`、`make rv_qemu_run`、`make kernel-la`、`make la_qemu_run`、PC watch / symbol_at 等
- 说明两套 target triple 与 feature（`qemu-riscv64-opensbi` / `qemu-loongarch64-virt`）
- QEMU 镜像、sdcard、日志保存目标（如 `rv_qemu_run_with_log`）— 以 `os/Makefile` 为准

### 功能验证

- 启动期自检（MM、driver、fs、vfs）日志关键字
- `user_bringup_bus`、busybox 脚本路径、glibc/musl 分组
- `longtable`：验证入口 | 覆盖内容 | 日志特征
- 提及 `parse_qemu_test_log.py` 若仓库存在

### 双架构一致性验证

- 对比表：构建产物、启动入口、内存路径、用户态行为一致性
- 强调：差异应在平台层可见，上层 syscall 行为应一致

### 遇到的问题和解决方法

按 **主题** `\subsection`，建议至少覆盖：

- 系统调用语义和错误码
- 文件系统和 VFS 路径
- 并发、锁和资源回收
- 网络与双架构差异
- 定位和复现方法（日志、pc_watch、symbol_at）

每条问题写：**现象、原因、处理** 三段，可引用真实排障经验，勿编造未发生过的 bug 细节

## 事实来源

- `os/Makefile`、根 `Makefile`
- `os/src/user_bringup_*.rs`、`docs/guides/task-board.md`
- `tem/` 或文档中的测例日志（仅作格式示例，注明路径）
- 第 1、2 章已写明的 bring-up 能力（勿矛盾）

## 禁止

- 不写未在 Makefile 中存在的目标名
- 不把第 3 章实现细节再展开一遍（问题节只写与排障相关的切面）

## 完成检查

- [ ] 所有命令可在当前仓库 Makefile 中找到
- [ ] 日志特征字符串与真实启动输出核对过
- [ ] 双架构表与第 2 章一致
- [ ] 文件顶部 `%` 说明注释保留
