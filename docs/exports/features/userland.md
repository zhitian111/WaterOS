# userland — 已实现功能

事实来源：`user/Cargo.toml`、`user/src/lib.rs`、`user/Makefile`、子模块 `wateros_user_mode_program`（路径 `user/`）。

## 用途

为 WaterOS 内核 bring-up 与 LTP/竞赛测例提供 **RISC-V 64 用户态**静态 ELF：公共运行时库、编号化烟测 bin 与交互式 shell/init 进程。

## Crate 与构建

| 项 | 状态 | 说明 |
|----|------|------|
| `wateros_user_mode_program` | 已实现 | 根 crate；库名 `wateros_user_lib` |
| 目标三元组 | RISC-V only | `riscv64gc-unknown-none-elf`（`Makefile` / `.cargo/config.toml`） |
| `build.rs` | 已实现 | 监听 `linker.ld` 变更，保证入口/BSS 符号与 Rust 侧一致 |
| `Makefile` | 已实现 | `rv_all`：cargo build → objcopy 二进制/ELF → `rv_disk.img` ext4 镜像 |
| LoongArch 用户态 | 未实现 | 仅 `src/riscv/` 后端；无 `loongarch` 目录 |

## 库能力（`wateros_user_lib`）

| 模块 | 状态 | 说明 |
|------|------|------|
| `_start` 入口 | 已实现 | `.text.entry`：清 BSS → `init_heap` → `main` → `exit` |
| 弱符号 `main` | 已实现 | 各 bin 覆盖；缺省 `panic!` |
| `share::console` | 已实现 | `print!`/`println!`、`prints`、`getchar`（fd 0/1） |
| `share::heap_allocator` | 已实现 | 16 MiB 伙伴堆（`USER_HEAP_SIZE_BITS_WIDTH=24`） |
| `share::syscall` | 已实现 | 架构无关薄封装，当前 `#[cfg(riscv64)]` 委托 `riscv::syscall` |
| `riscv::syscall` | 已实现 | `ecall` + Linux 兼容调试号表（与内核 `wateros-abi` 对齐） |
| `riscv::lang_items` | 已实现 | `panic_handler` 打印位置后 `unreachable!` |
| `riscv::clear_bss` | 已实现 | 使用 `linker.ld` 导出 `bss_start`/`bss_end` |

## 已封装 syscall（用户库层）

| 用户 API | 内核号（RISC-V 路径） | 状态 |
|----------|----------------------|------|
| `write` / `read` | 64 / 63 | 已实现 |
| `exit` | 93 | 已实现 |
| `yield_` | 124 | 已实现 |
| `get_time` | 169 | 已实现 |
| `brk` | 214 | 已实现 |
| `uname` | 160 | 已实现 |
| `fork` | 220 | 已实现 |
| `wait` / `waitpid` | 260 | 已实现（`-2` 时轮询 `yield_`） |
| `exec` | 221 | 已实现 |

## 用户程序（`src/bin/`）

| Bin | 状态 | 说明 |
|-----|------|------|
| `000_hello_world` | 已实现 | 控制台输出烟测 |
| `001_power` | 已实现 | 模幂 CPU 负载 |
| `002_store_fault` | 已实现 | 故意空指针写，期望内核杀进程 |
| `003_sleep` | 已实现 | `get_time` + `yield_` 协作延时 |
| `004_power_3` / `005_power_5` / `006_power_7` | 已实现 | 长循环负载系列 |
| `007_brk` | 已实现 | `brk` 探测；**未**写入根 `Cargo.toml` `[[bin]]`，需显式路径构建 |
| `008_initproc` | 已实现 | `fork` + `exec("user_shell\0")`，父进程 `wait` 回收 |
| `009_user_shell` | 已实现 | 行编辑 + `fork`/`exec`/`waitpid` 交互 shell |

构建产物经 `script/rv_gen_ext4_disk_img.sh` 打入 `rv_disk.img`，供内核根卷 `/elf/*.elf` 加载。

## 与内核关系

- 子模块：`.gitmodules` 指向 `wateros_user_mode_program`；本地须 `git submodule update --init user` 或克隆到 `user/`。
- RISC-V 主线：`os/src/main.rs` bring-up 可从根卷加载 `/elf/000_hello_world.elf` 等用户任务。
- syscall 编号须与 `wateros-abi` `impl-linux-generic64` 及内核 trap 解码一致。

## 缺口

- 无 LoongArch64 用户态链接脚本与 `ecall` 后端
- `share::syscall` 无 `#[cfg(not(riscv64))]` 占位，非 RISC-V 目标无法编译
- `007_brk` 未注册为正式 `[[bin]]`
- 用户堆固定 16 MiB，无运行时扩缩
- `exec` 路径约定为 C 字符串（含 `\0`），无参数向量/环境变量封装
- 子模块 URL 若被本地 `git config submodule.user.url` 覆盖为空目录，需手动克隆

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释 / `#[inline]` / 文档任务同步） |
