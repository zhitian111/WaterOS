# os 内核二进制 — 对外符号与入口

## 用途

列出根 crate `wateros` 在链接后对外可见的 **全局符号** 与 **crate 内可调用入口**（供 bring-up、自检或其它 crate 通过路径引用）。本 crate 无 `lib.rs`，**不提供** `rlib` 式再导出。

事实来源：`os/src/**`、`nm`/`readelf` 对最终 `wateros` 二进制的预期符号。

## 链接可见符号（C ABI / 全局 handler）

| 符号 | 类型 | 说明 |
|------|------|------|
| `_start` | 汇编 | `link.ld` `ENTRY`；平台 `_start.S` |
| `kernel_main` | `extern "C" fn(...) -> !` | 板级 Rust 入口；riscv64 为 `(boot_arg0, boot_arg1)`，loongarch64 为 `(_argc, _argv, envp)` |
| `panic_handler` | `#[panic_handler]` | 委托 `runtime::panic::panic_handler` |
| `alloc_error_handler` | `#[alloc_error_handler]` | 委托 `runtime::heap_allocator::handle_alloc_error` |
| `kernel_end` | 链接脚本符号 | MM 自检 frame 起点（`extern "C"` 引用） |

平台 trap 向量、syscall 跳板等在 `wateros-platform-arch` 汇编中导出，不在此 crate 定义。

## Crate 内模块 API（`os/src`，按路径调用）

### `boot_timebase`

| 项 | 签名 / 说明 |
|----|-------------|
| `probe_and_init_timebase` | `fn(dtb_pa: usize) -> u64` — 探测并设置 timebase，返回采用频率 |

### `trap_handler`

| 项 | 签名 / 说明 |
|----|-------------|
| `init` | `fn()` — 注册 `wateros_kernel_trap_handler`；须在 `task::init()` 之后 |

### `user_bringup_bus`

| 项 | 签名 / 说明 |
|----|-------------|
| `run` | `fn()` — bring-up 总线主入口 |

### `user_bringup_common`

| 项 | 说明 |
|----|------|
| `LIBC_PREFIXES` | `&[&str]` — 默认 `["/glibc"]` |
| `BringupCommand` | `{ program, argv }` — busybox 阶段命令描述 |
| `run_one_bringup_command` | 串行执行一条 `BringupCommand` |
| `spawn_user_task_from_loaded_elf_with_argv` | 已装载 ELF + argv/envp → `TaskId` |
| `run_one_elf_argv` / `run_one_elf_argv_exit` | 单 ELF/脚本装载执行 |
| `run_one_basic_elf` | `/{prefix}/basic/{name}` 快捷路径 |
| `run_one_busybox_script` | `busybox sh <script>` 快捷路径 |

### `user_bringup_basic` / `user_bringup_busybox` / `user_bringup_mm` / `user_bringup_posix_fs`

| 模块 | 入口 |
|------|------|
| `user_bringup_basic` | `run_stage_basic()` |
| `user_bringup_busybox` | `run_stage_busybox()` |
| `user_bringup_mm` | `run_stage_02()` |
| `user_bringup_posix_fs` | `run_stage_posix_fs_meta()` |

### `user_bringup_root_layout`

| 项 | 说明 |
|----|------|
| `ensure_busybox_path_links` | RW 根卷上创建目录与 busybox 硬链接 |
| `refresh_ltp_accounts` | 重写 `/etc/passwd` 等（LTP 用） |

### `self_tests`

| 模块 | 入口 | 说明 |
|------|------|------|
| `self_tests::network` | `run_sync_smoke()`, `spawn_all()` | 同步烟测；`spawn_all` 为空 |
| `self_tests::task` | `spawn_all()` | 仅 riscv64；当前禁用 |

## 初始化契约（调用顺序）

由 `kernel_main` 保证；外部 **不应** 跳过或重排以下关键边：

1. `runtime::heap_allocator::init` — 任何 `alloc` 前  
2. `task::init` → `trap_handler::init` — 任何 satp/用户 trap 前  
3. `mm::kernel_mm::init` — 用户 ELF 装载前  
4. `fs::init` → `user_bringup_bus::run` — 用户路径访问前须 RW 根卷（总线内挂载）  
5. `task::run_first_task` — 所有 spawn 登记完成后  

## 依赖的一级组件（非本 crate API）

`kernel_main` 直接调用：`platform`、`runtime`、`klog`、`driver`、`mm`、`task`、`fs`、`vfs`（feature）、`syscall`（经 trap）、`cred`（bring-up spawn）。其公共 API 见 `docs/exports/public-api/wateros-*.md`。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
