# 编码规范 Prompt

本文件按文件角色定义 WaterOS 的编码要求，而不是按文件后缀名分类。

## 1. Workspace 与组件入口

适用范围：根 `Cargo.toml`、一级组件 `Cargo.toml`、子组件 `Cargo.toml`。

要求：

- feature 命名必须体现语义，例如 `api-v0`、`impl-sv39`、`impl-qemu-riscv64-opensbi`。
- 默认 feature 只能指向当前项目认可的默认实现。
- 依赖关系应明确体现 API 与 impl 的连接，不要隐式耦合。
- 新增 impl 时必须同时补齐 workspace members、依赖和 feature 传递链。

## 2. 聚合门面

适用范围：各一级组件或子组件的 `src/lib.rs`。

要求：

- 聚合层负责统一导出，不直接堆叠实现细节。
- 通过 `#[cfg(feature = ...)]` 选择实现时，对外名称应保持稳定。
- 对外函数、类型别名和模块名要体现该组件最终提供的能力。
- 聚合层可以包含少量包装逻辑，但不要把复杂实现塞入门面层。
- 若组件需要自检或 bring-up 验证，应优先在聚合层提供统一测试入口，例如 `test()`、`test_with_range(...)`，由该入口串联 API 层测试、子组件测试和当前激活 impl 的测试。
- 测试入口的命名应体现输入语义；无额外输入时优先使用 `test()`，需要上下文时使用类似 `test_with_range(...)` 的显式命名。
- 不同 impl 不应各自发散出完全不同的测试入口名，统一入口由聚合层负责收敛。

## 3. API 定义层

适用范围：`*-api/api-v0/src/*.rs`。

要求：

- API 层只定义契约，不实现具体平台逻辑。
- 优先定义 trait、newtype、错误类型、常量和文档契约。
- 对外可见项应具备 `///` 文档注释。
- API 的命名要稳定、可扩展、便于 impl 实现。

## 4. impl 实现层

适用范围：`*-impl/impl-*/src/*.rs`。

要求：

- impl 层严格围绕 API 契约实现，不擅自扩大边界。
- 临时实现和占位实现必须明确标注局限性。
- 平台相关、算法相关、硬件相关代码应集中放在 impl 层。
- 新增 impl 时优先保持与现有目录命名风格一致。

## 5. 底层 Rust 实现

适用范围：platform、arch、runtime、mm 等底层 Rust 文件。

要求：

- 默认假设为 `no_std` 环境。
- 需使用 `alloc` 时显式说明原因。
- 涉及地址、寄存器、页表、Trap、固件接口时，应优先使用明确的类型而非裸值。
- 避免把 bring-up 测试逻辑与最终长期接口混在一起。

### 测试接口约定

- WaterOS 当前优先使用“统一测试接口”做组件级自检，而不是让上层直接调用某个 impl 内部测试函数。
- 典型模式是：聚合层暴露 `test()` 或 `test_with_xxx(...)`，内部再按顺序调用 API 层测试、子组件测试和当前激活 impl 的测试。
- 组件测试入口应尽量可被 `os/src/main.rs` 或更上层聚合路径直接调用，方便在 bring-up 阶段形成稳定的自检链。
- 若某个 impl 暂无真实测试逻辑，也应在统一入口中明确记录“跳过原因”，而不是静默不测。

### Logging 约定

- 日志输出应统一基于 `wateros-runtime/runtime-logging` 提供的初始化与宏导出能力。
- 组件代码中应优先使用统一的日志宏风格，例如 `trace!`、`debug!`、`info!`、`warn!`、`error!`，不要自行封装另一套不兼容接口。
- 日志前缀应体现组件身份，推荐使用类似 `[driver]`、`[wateros-mm]` 的固定标签，便于串联启动与测试日志。
- `trace!` 适合记录测试开始、结束和细粒度路径；`debug!` 适合记录关键中间状态；`info!` 适合记录阶段性成功或降级说明；`warn!` 适合记录可恢复异常；`error!` 仅用于明确错误路径。
- 日志内容应优先描述上下文和结果，不要只输出无语义的数值或“到此一游”式文本。
- 测试入口中的日志应成对出现，至少能看出 begin/end 或成功/跳过关系，便于快速定位哪个组件自检失败。

## 6. 构建入口

适用范围：`os/Makefile`、`build.rs`。

**所有内核编译、QEMU 运行、架构 check 均通过 `os/Makefile` 调用**（见 `general.md`「构建与运行」）。Agent 在任务中需要 build/run 时必须 `cd os` 后执行 make 目标，不得默认使用裸 `cargo build` / 手写 qemu 命令替代。

### `os/Makefile` 目标速查

工作目录：`cd os`。

| 分类 | 目标 | 说明 |
|------|------|------|
| 构建 | `kernel-rv` | riscv64 内核 → `./kernel-rv` |
| 构建 | `kernel-la` | loongarch64 内核 → `./kernel-la` |
| 构建 | `all` | `kernel-rv` + strip 镜像 |
| 运行 | `rv_qemu_run` | 编译并 QEMU 运行 RISC-V（bring-up / 测例） |
| 运行 | `la_qemu_run` | QEMU 运行 LoongArch |
| 运行 | `rv_qemu_run_with_log` | RISC-V + QEMU int/cpu 日志 |
| 调试 | `<run-target>-gdb` | 开放 GDB 端口并暂停等待连接，例如 `rv_pre_run-gdb` |
| 检查 | `rv_check` / `la_check` / `check` | cargo check（feature 已对齐） |
| 分析 | `rv_elf_info` / `la_elf_info` | readelf 内核 ELF |
| 维护 | `clean` | 清理构建产物 |
| 维护 | `flush_img` | 从 `../test_case/` 拷贝 sdcard 镜像 |
| 维护 | `fmt` | taplo + rustfmt |
| 其他 | `version` / `stat` | 版本与仓库统计 |

底层脚本：`os/scripts/rv_pre_run.sh`、`rv_final_run.sh`、`rv_qemu_run_with_log.sh`
等；修改 QEMU 参数时同步检查 `docs/prompts/tasks/run_testsuits_qemu.md` 中的环境说明。

要求：

- 命令命名应稳定、可读、可组合。
- 自动化入口应尽量可复现，不依赖模糊的本地状态。
- 变更构建参数时，应同步检查文档和脚本说明。
- 新增 make 目标或变更行为时，同步更新 `general.md` 本表与相关 task 文档。

## 7. 自动化脚本

适用范围：`os/scripts/*`、`user/script/*`。

要求：

- 脚本应说明输入、输出和使用场景。
- 涉及生成物时，优先说明生成物写入位置。
- 若脚本用于导出项目结构、feature 树或运行环境，应同步反映到相关文档。

## 8. Markdown 文档

适用范围：`docs/**/*.md`。

要求：

- 文档要以事实为基础，不凭目录名猜测能力。
- 涉及接口、实现、feature 时应引用真实路径或真实 crate 名。
- 输出应优先按组件拆分，避免单个文档过大。

## 继承的仓库风格

- 当前项目使用较长但语义清晰的命名。
- 当前项目允许通过聚合 crate 统一导出接口。
- 当前项目已经接受 `api-v0` 与 `impl-*` 这类显式分层。
- Rust 格式应尊重仓库现有配置，不私自切换到另一套风格。
