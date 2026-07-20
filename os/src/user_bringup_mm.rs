//! `stage-02-mm`：在根卷已挂载后，从 **`/glibc/basic/`** 与 **`/musl/basic/`**
//! 加载 MM 相关测程 ELF 并 `spawn` 用户任务（并行入队，不等待退出）。
//!
//! `brk` / `mmap` / `munmap` 与 [`crate::user_bringup_basic`] 的 syscall 子集分离，
//! 避免重复装载。须在 `fs::init` 之后调用。

use runtime::logging::*;

/// 默认尝试的 MM 子集路径（可按镜像增量增删）。
#[allow(unused)]
const MM_GLIBC_PATHS : &[&str] = &["/glibc/basic/brk",
                                   "/glibc/basic/mmap",
                                   "/glibc/basic/munmap"];
#[allow(unused)]
const MM_MUSL_PATHS : &[&str] = &[
    // "/musl/basic/brk",
    // "/musl/basic/mmap",
    // "/musl/basic/munmap",
];
