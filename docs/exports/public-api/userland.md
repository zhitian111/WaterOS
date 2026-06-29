# userland — 公共 API

事实来源：`user/src/lib.rs`、`user/src/share/*.rs`；库 crate 名 **`wateros_user_lib`**。

## 启用与链接

- 各 `src/bin/*.rs` 以 `#![no_std]` `#![no_main]` 链接本库；入口由库提供 `_start`，bin 仅实现 `main`。
- 需要 `alloc` 的 bin（如 `009_user_shell`）须 `extern crate alloc` 并启用全局分配器（`_start` 已 `init_heap`）。

## 根库导出（`wateros_user_lib`）

### 入口与生命周期

| 符号 | 属性 | 说明 |
|------|------|------|
| `_start` | `extern "C"`, `.text.entry` | 用户镜像唯一入口；**无** `#[inline]` |
| `main` | `#[linkage = "weak"]` | 默认 `panic!("Cannot find main!")`；bin 以 `#[no_mangle] fn main() -> i32` 覆盖 |

### 进程与 I/O 封装（均带 `#[inline]`）

```rust
pub fn write(fd: usize, buf: &[u8]) -> isize
pub fn read(fd: usize, buffer: &mut [u8]) -> isize
pub fn exit(exit_code: i32) -> isize
pub fn yield_() -> isize
pub fn get_time() -> isize
pub fn brk(addr: usize) -> isize
pub fn uname(addr: usize) -> isize
pub fn fork() -> isize
pub fn exec(path: &str) -> isize
pub fn wait(exit_code: &mut i32) -> isize
pub fn waitpid(pid: usize, exit_code: &mut i32) -> isize
```

### 再导出

| 路径 | 说明 |
|------|------|
| `share` | 子模块树（见下） |
| `print` | `share::console::print`，供 `print!` / `println!` 宏 |

### 导出宏（`#[macro_export]`，crate 根）

- `print!`、`println!` — 定义于 `share/console.rs`

## `share` 子模块

### `share::config`

| 名称 | 类型 | 说明 |
|------|------|------|
| `USER_HEAP_SIZE_BITS_WIDTH` | `const usize` | 伙伴堆阶数，当前 `24` |
| `USER_HEAP_SIZE` | `const usize` | `1 << USER_HEAP_SIZE_BITS_WIDTH` 字节 |

### `share::console`

| 名称 | `#[inline]` | 说明 |
|------|-------------|------|
| `Stdout` | — | `fmt::Write` → fd 1 |
| `print` | 是 | `fmt::Arguments` → stdout |
| `prints` | 是 | 写 `&str` |
| `getchar` | 是 | 阻塞读 fd 0 一字节 |

### `share::heap_allocator`

| 名称 | `#[inline]` | 说明 |
|------|-------------|------|
| `init_heap` | 是 | 注册静态 `HEAP_SPACE`；`_start` 调用 |
| `handle_alloc_error` | — | `#[alloc_error_handler]`，分配失败 panic |
| `heap_test` | — | 开发烟测，`#[allow(unused)]` |

### `share::syscall`

与根库同名 `sys_*` 薄封装，均 `#[inline]`，当前仅 RISC-V 64 有实现：

`sys_write`, `sys_exit`, `sys_yield`, `sys_get_time`, `sys_brk`, `sys_uname`, `sys_fork`, `sys_waitpid`, `sys_exec`, `sys_read`

## `riscv` 子模块（`pub` 通过 `mod riscv` 未重导出，经 `share::syscall` 使用）

| 名称 | `#[inline]` | 说明 |
|------|-------------|------|
| `clear_bss` | 是 | 清零 `.bss` |
| `syscall::sys_*` | 是 | `ecall` 发起；私有 `syscall(id, [a0,a1,a2])` 亦 inline |

### `riscv::lang_items`

- `panic_handler` — 用户 panic 打印；**无** `#[inline]`

## 链接脚本符号（`linker.ld`，非 Rust API）

| 符号 | 用途 |
|------|------|
| `_start` / `.text.entry` | 与 Rust `_start` 对应 |
| `bss_start` / `bss_end` | `clear_bss`、`heap_test` 范围断言 |
| `text_start` / `rodata_*` / `data_*` | 段界调试（当前 Rust 未直接引用） |

## 依赖

```toml
buddy_system_allocator = "0.11.0"
```

- `#![no_std]` + `alloc`（堆与 shell）
- `profile.release.panic = "abort"`

## 未导出 / 需注意

- 无 `std`、无 `libc`；字符串路径须 NUL 结尾才能 `exec`
- `wait`/`waitpid` 对内核返回 `-2` 的行为是用户库策略，非 Linux 阻塞语义
- syscall 号硬编码于 `riscv/syscall.rs`，变更须同步内核 `wateros-abi`
- `007_brk` 源文件存在但未在 `Cargo.toml` 声明 `[[bin]]`

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
