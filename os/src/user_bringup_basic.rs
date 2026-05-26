//! `stage-03-basic`：在根卷已挂载后，从 **`/glibc/basic/`** 与
//! **`/musl/basic/`** 加载 syscall/FS 等基础测程 ELF 并 `spawn` 用户任务（与
//! `os/tem/glibc/basic`、`os/tem/musl/basic` 等镜像布局一致；缺失文件则 `warn`
//! 跳过）。
//!
//! `brk` / `mmap` / `munmap` 由 [`crate::user_bringup_mm::run_stage_02`]
//! 登记，此处不重复装载。 须在 `fs::init` 之后调用；用户态实际执行在
//! `task::run_first_task()` 之后。

use runtime::logging::*;

/// 生成某 libc 前缀下的基础测程路径表（不含 MM 三者）。
macro_rules! basic_elf_paths {
    ($prefix:literal) => {
        &[concat!($prefix, "/chdir"),
          concat!($prefix, "/clone"),
          concat!($prefix, "/execve"),
          // concat!($prefix, "/close"),
          // concat!($prefix, "/dup"),
          // concat!($prefix, "/dup2"),
          // concat!($prefix, "/execve"),
          //concat!($prefix, "/exit"),
          //concat!($prefix, "/fork"),
          // concat!($prefix, "/fstat"),
          // concat!($prefix, "/getcwd"),
          // concat!($prefix, "/getdents"),
          // concat!($prefix, "/getpid"),
          // concat!($prefix, "/getppid"),
          // concat!($prefix, "/gettimeofday"),
          // concat!($prefix, "/mkdir_"),
          // concat!($prefix, "/mnt"),
          // concat!($prefix, "/mount"),
          // concat!($prefix, "/open"),
          // concat!($prefix, "/openat"),
          // concat!($prefix, "/pipe"),
          // concat!($prefix, "/read"),
          // concat!($prefix, "/sleep"),
          // concat!($prefix, "/test_echo"),
          // concat!($prefix, "/times"),
          // concat!($prefix, "/umount"),
          // concat!($prefix, "/uname"),
          // concat!($prefix, "/unlink"),
          // concat!($prefix, "/wait"),
          // concat!($prefix, "/waitpid"),
          //concat!($prefix, "/write") /* concat!($prefix, "/yield"), */
          ]
    };
}

const BASIC_GLIBC_PATHS : &[&str] = basic_elf_paths!("/glibc/basic");
const BASIC_MUSL_PATHS : &[&str] = basic_elf_paths!("/musl/basic");

/// 启动期检查 oscomp basic 测例依赖的根卷路径。
#[cfg(feature = "vfs-bridge")]
fn warn_missing_basic_assets() {
    use vfs::api::SingleRootReadView;
    let view = vfs::root::read_view();
    for path in ["/glibc/basic/text.txt",
                 "/glibc/basic/mnt",
                 "/musl/basic/text.txt",
                 "/musl/basic/mnt"]
    {
        match view.exists(path) {
            Ok(true) => info!("[basic-bringup] rootfs asset present: {}",
                              path),
            Ok(false) => warn!("[basic-bringup] rootfs asset MISSING: {} (oscomp fstat/openat \
                                may fail)",
                               path),
            Err(e) => warn!("[basic-bringup] rootfs check {}: {:?}",
                            path, e),
        }
    }
}

/// 执行 `stage-03-basic`：装载并登记用户测程。
pub fn run_stage_03() {
    info!("[bringup][stage-03-basic] BEGIN");
    #[cfg(not(feature = "impl-sv39"))]
    {
        warn!("[basic-bringup] impl-sv39 off: skip glibc/musl basic ELF load");
        info!("[bringup][stage-03-basic] END");
        return;
    }
    #[cfg(feature = "impl-sv39")]
    {
        let n = BASIC_GLIBC_PATHS.len() + BASIC_MUSL_PATHS.len();
        info!("[basic-bringup] will try {} ELF(s) under /glibc/basic/ and /musl/basic/ \
               (brk/mmap/munmap via stage-02-mm)",
              n);
        info!("[basic-bringup] spawn only enqueues user tasks; CPU-side user code runs after \
               task::run_first_task()");
        #[cfg(feature = "vfs-bridge")]
        warn_missing_basic_assets();
        for path in BASIC_GLIBC_PATHS.iter()
                                     .chain(BASIC_MUSL_PATHS)
        {
            match mm::kernel_mm::from_elf_path(path) {
                Ok(loaded) => {
                    info!("[basic-bringup] loaded path={} entry={:#x} image=[{:#x},+{:#x}) \
                           stack=[{:#x},{:#x}) brk=[{:#x},{:#x}) mmap_base={:#x} aspace_ptr={:#x}",
                          path,
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
                    vfs::cwd::on_user_task_spawned_for_elf(tid, path);
                    info!("[basic-bringup] spawned user task {} for {}",
                          tid, path);
                }
                Err(e) => {
                    warn!("[basic-bringup] skip path={}: {:?}",
                          path, e);
                }
            }
        }
    }
    info!("[bringup][stage-03-basic] END");
}
