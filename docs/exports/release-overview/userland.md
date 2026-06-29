# userland — 版本概述

面向 bring-up、课堂演示与 OS 竞赛测例的 **RISC-V 用户态程序集**概述。细节以 `user/` 源码与 `docs/exports/features/userland.md` 为准。

## 当前阶段能做什么

本阶段用户态侧已具备一套可重复构建的 **最小用户运行时** 与 **编号化测试程序**，能够在 WaterOS 内核上完成：

- 从 ELF 入口启动、清 BSS、初始化伙伴堆并进入用户 `main`
- 通过 `ecall` 使用一小套 Linux 兼容调试 syscall（读写控制台、退出、让出 CPU、时间、brk、uname、fork/exec/waitpid）
- 运行 hello、CPU 负载、故意页错、协作式 sleep、brk 探测等烟测
- 运行简易 **initproc + 交互 shell**（行编辑、`fork`/`exec` 子命令、`waitpid` 回收）

构建侧可通过 `user/Makefile` 的 `rv_all` 一键生成 RISC-V 二进制、ELF 及 **ext4 测试磁盘镜像** `rv_disk.img`，供内核挂载为根卷并从 `/elf/` 加载用户程序。

## 适用范围

| 场景 | 是否适用 |
|------|----------|
| QEMU `riscv64gc-unknown-none-elf` + WaterOS RISC-V 主线 | 是 |
| LoongArch64 用户态程序 | 否（尚未提供 LA 链接与 syscall 后端） |
| 完整 Linux 用户态 / glibc 程序 | 否 |
| LTP / 竞赛 syscall 子集验证 | 部分（依赖内核已实现对应 syscall） |

## 版本与仓库关系

- 用户态独立仓库：`wateros_user_mode_program`（父仓库 Git 子模块 `user/`）。
- 版本脚本：`user/Makefile` `version` 目标（`v0.1.0-prototype.<git-count>+branch`）。
- 与内核版本对应关系见子模块 `user/README.md`（如 prototype.10 起 ELF 基址按 index 偏移等历史约定）。

## 刻意未包含的能力

以下能力**不在**当前用户态发布范围内，避免与内核未完成项产生误解：

- LoongArch64 用户镜像与调用约定
- 动态链接、线程库、完整 POSIX libc
- `exec` 参数向量 / 环境变量、`pipe`/`mmap` 等尚未在用户库封装的 syscall
- 与内核独立的 syscall 号表 crate（号表仍硬编码在 `riscv/syscall.rs`，需与内核同步维护）

## 使用方式（摘要）

1. 初始化子模块：`git submodule update --init user`（若 `user/` 为空）。
2. 构建：`cd user && make rv_all`（或 `make check` 仅检查）。
3. 将 `rv_disk.img` 供内核/QEMU 使用（参见 `os/Makefile` 与 `test_case` 文档）。
4. 内核 bring-up 从根卷加载 `/elf/000_hello_world.elf` 或启动 initproc/shell 路径。

## 下一阶段方向（概述）

- LoongArch64 用户态后端与链接脚本
- 将 syscall 号表改为依赖或生成自 `wateros-abi`，减少双份维护
- 补齐 `007_brk` 的 `Cargo.toml` `[[bin]]` 注册
- 扩展用户库 syscall 封装以覆盖内核已实现的文件/进程/网络子集

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版版本概述 |
