//! `stage-02-mm`：在根卷已挂载后，从 **`/glibc/basic/`** 与 **`/musl/basic/`**
//! 加载 MM 相关测程 ELF 并 `spawn` 用户任务（并行入队，不等待退出）。
//!
//! `brk` / `mmap` / `munmap` 与 [`crate::user_bringup_basic`] 的 syscall 子集分离，
//! 避免重复装载。须在 `fs::init` 之后调用。

use runtime::logging::*;

/// 默认尝试的 MM 子集路径（可按镜像增量增删）。
const MM_GLIBC_PATHS : &[&str] = &["/glibc/basic/brk",
                                   "/glibc/basic/mmap",
                                   "/glibc/basic/munmap"];
const MM_MUSL_PATHS : &[&str] = &[
    // "/musl/basic/brk",
    // "/musl/basic/mmap",
    // "/musl/basic/munmap",
];

/// 执行 `stage-02-mm`：装载并登记用户测程（并行 spawn）。
pub fn run_stage_02() {
    info!("[bringup][stage-02-mm] BEGIN");
    let n = MM_GLIBC_PATHS.len() + MM_MUSL_PATHS.len();
    info!("[mm-bringup] will try {n} ELF(s) under /glibc/basic/ and /musl/basic/");
    info!("[mm-bringup] spawn only enqueues user tasks; CPU-side user code runs after \
           task::run_first_task()");
    for path in MM_GLIBC_PATHS.iter()
                              .chain(MM_MUSL_PATHS)
    {
        match mm::kernel_mm::from_elf_path(path) {
            Ok(loaded) => {
                info!("[mm-bringup] loaded path={path} entry={:#x} image=[{:#x},+{:#x}) \
                       stack=[{:#x},{:#x}) brk=[{:#x},{:#x}) mmap_base={:#x} aspace_ptr={:#x}",
                      loaded.entry_pc,
                      loaded.image_base,
                      loaded.image_size,
                      loaded.stack_bottom,
                      loaded.stack_top,
                      loaded.brk_start,
                      loaded.brk_max,
                      loaded.mmap_arena_base,
                      loaded.user_aspace_ptr);
                let tid = task::spawn_user_task_from_loaded_elf(&loaded);
                #[cfg(feature = "vfs-bridge")]
                vfs::cwd::on_user_task_spawned_for_elf(tid, path, &[path]);
                info!("[mm-bringup] spawned user task {tid} for {path}");
            }
            Err(e) => {
                warn!("[mm-bringup] skip path={path}: {e:?}");
            }
        }
    }
    info!("[bringup][stage-02-mm] END");
}
