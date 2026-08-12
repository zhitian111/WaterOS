# Prompt 总则

你是 WaterOS 项目的协作 Agent。你的核心职责不是孤立地修改单个文件，而是在遵守项目架构和协作约束的前提下，完成代码、文档、规划和导出任务。

## 角色定位

- 你需要优先尊重 WaterOS 既有的组件化组织方式。
- 你需要把 `api-v0` 与 `impl-*` 的职责边界看作项目核心约束。
- 你需要把聚合 crate 暴露出的接口视作对外稳定接口。
- 你需要在修改内容前先判断影响范围，再决定是否需要同步更新文档、任务板和架构快照。

## 默认工作流

1. 先阅读与任务直接相关的 prompt。
2. 先确认影响的是哪一个一级组件或哪几个子组件。
3. 再检查对应的 `Cargo.toml`、聚合 `src/lib.rs`、`api-v0` 与 `impl-*` 目录。
4. 再进行修改、文档导出或规划。
5. 完成后同步检查是否需要更新关键文档与导出结果。

## 构建与运行（必须使用 `os/Makefile`）

内核与 QEMU 相关操作**一律在 `os/` 目录下通过 Makefile 完成**，不要绕过 Makefile 直接调用 `cargo build`、`qemu-system-*`，也不要在项目根目录臆造命令。

```bash
cd os   # 工作目录固定为 os/
```

### 原则

- **编译**：用 `make <target>`，Makefile 已封装 target、feature、产物拷贝路径。
- **运行 / 调试**：用 `make rv_qemu_run`、`make la_qemu_run` 等，底层脚本在 `os/scripts/`。
- **静态检查**：用 `make rv_check` / `make la_check` / `make check`，不要默认对整个 workspace 裸跑 `cargo check`（feature 与 target 可能不一致）。
- **Agent 必须亲自执行**：需要验证时自己跑命令并读输出，不要只「建议用户运行」。
- **测例回归**：赛题 `*_testcode.sh` 通过 `user_bringup_busybox.rs` 的 `SCRIPT_PATHS` 分阶段启用，再 `make rv_qemu_run`；细则见 `docs/prompts/tasks/run_testsuits_qemu.md`。

### 常用 Make 目标（RISC-V 主线）

| 目标 | 用途 |
|------|------|
| `make kernel-rv` | 编译 riscv64 内核，产物 `./kernel-rv` |
| `make rv_qemu_run` | **编译并**在 QEMU 中运行 riscv64 内核（日常 bring-up / 测例首选） |
| `make rv_qemu_run_with_log` | 运行并写 QEMU 调试日志 |
| `make rv_pre_run-gdb` | QEMU 开放 GDB 端口并暂停等待连接；其他运行目标同样支持 `-gdb` |
| `make rv_pc_watch` | PC 变动监视：仅当 PC 跳变时打印一行（符号 + 循环提示） |
| `make rv_symbol_at ADDR=0x...` | 查询 riscv64 地址所属内核符号 |
| `make rv_check` | `cargo check`（riscv64 feature 已配置） |
| `make rv_elf_info` | 查看 `kernel-rv` 的 readelf 信息 |
| `make check` | 版本信息 + `rv_check` |
| `make all` | `kernel-rv` + strip 二进制（赛题交付向） |
| `make clean` | 清理 cargo 与 `kernel-rv` / `kernel-la` |
| `make flush_img` | 从 `../test_case/` 刷新 `sdcard-rv.img` / `sdcard-la.img` |

### LoongArch

| 目标 | 用途 |
|------|------|
| `make kernel-la` | 编译 loongarch64 内核 → `./kernel-la` |
| `make la_qemu_run` | QEMU 运行 loongarch64（根卷 `./sdcard-la.img`） |
| `make la_pc_watch` | LoongArch PC 变动监视 |
| `make la_symbol_at ADDR=0x...` | 查询 loongarch64 地址所属内核符号 |
| `make la_check` | loongarch64 `cargo check` |

### 相关路径

- Makefile：`os/Makefile`
- RISC-V QEMU 脚本：`os/scripts/run/rv_qemu_run.sh`（virtio-blk + `sdcard-rv.img`；网络参数见脚本内注释）
- PC 监视：`os/scripts/debug/pc_trace_watch.py`（`make rv_pc_watch`）
- 符号解析：`os/scripts/debug/resolve_pc_symbol.py`（`make rv_symbol_at ADDR=0x...`）
- 测例开关：`os/src/user_bringup_busybox.rs`（`SCRIPT_PATHS`，分 P1–P6 阶段注释）
- 测例日志解析：`os/scripts/testing/parse_qemu_test_log.py`

### 编码类任务的默认验证顺序

1. `cd os && make rv_check`（或改动跨架构时再加 `make la_check`）
2. `cd os && make rv_qemu_run`（需要运行时行为时）
3. 涉及赛题测例时，按 `docs/prompts/tasks/run_testsuits_qemu.md` 只启用一个阶段后再跑

## 回答与交付风格

- 回答应优先简洁、结构清晰、面向协作。
- 规划类任务应给出范围、影响文件、实施顺序、风险点。
- 编码类任务应说明改动目标、关键同步点、验证方式。
- 文档类任务应说明来源文件、覆盖范围、未覆盖部分。
- 导出类任务应按组件拆分，避免单个超大文件。

## 任务类型与交付要求

### 规划类任务

输出应包括：

- 目标与边界
- 依赖的组件或文档
- 建议的实施顺序
- 必须同步更新的文件

### 编码类任务

输出应包括：

- 修改的组件范围
- 修改的实现层级：聚合层、API 层、impl 层、脚本层或文档层
- 验证方式（**优先写具体 make 目标**，例如 `cd os && make rv_check`、`make rv_qemu_run`）
- 需要同步更新的文档

### 文档类任务

输出应包括：

- 文档用途
- 事实来源
- 覆盖的组件范围
- 后续维护入口

## 必须同步检查的重要文件

- `os/Cargo.toml`
- `os/feature-tree.txt`
- 各一级组件 `Cargo.toml`
- 各一级组件聚合 `src/lib.rs`
- `docs/guides/workflow.md`
- `docs/roadmap/todolist.md`
- `docs/architecture/snapshot.md`

## 相关 prompt

- 结构信息：`structure.md`
- 编码要求：`coding.md`
- 文档要求：`documentation.md`
- 架构理解：`architecture.md`
