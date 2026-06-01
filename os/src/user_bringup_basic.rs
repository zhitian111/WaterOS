//! `stage-basic`：挂载根卷后登记内核 runner，串行执行
//! `/{glibc,musl}/basic/{test}` ELF，用于绕开 BusyBox/shell 依赖验证 syscall 路径。
//!
//! 测例名写在 [`BASIC_TESTS`] 中；当前先启用进程/线程生命周期小集合，
//! fd/cwd/io 与 mount 类测例待共享资源语义稳定后继续打开。

use runtime::logging::*;

/// basic ELF 测例名（对应 `/{prefix}/basic/{name}`）；按推荐顺序排列。
const BASIC_TESTS : &[&str] = &["clone",
                            //"brk"
                                //"fork",
                                //"wait",
                                //"waitpid",
                                //"getpid",
                                //"getppid",
                                //"exit",
                                //"execve",
                                // "chdir",
                                // "close",
                                // "dup",
                                // "dup2",
                                // "fstat",
                                // "getcwd",
                                // "getdents",
                                // "gettimeofday",
                                // "mkdir_",
                                // "mnt",
                                // "mount",
                                // "open",
                                // "openat",
                                // "pipe",
                                // "read",
                                // "sleep",
                                // "test_echo",
                                // "times",
                                // "umount",
                                // "uname",
                                // "unlink",
                                // "write",
                                // "yield",
                                ];

const LOG_TAG : &str = "basic-bringup";

/// 执行 `stage-basic`：登记内核串行 runner（不阻塞；用户态在 `run_first_task` 后运行）。
pub fn run_stage_basic() {
    info!("[bringup][stage-basic] BEGIN");
    #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
    {
        warn!("[{LOG_TAG}] no mm impl: skip");
        info!("[bringup][stage-basic] END");
        return;
    }
    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    {
        task::spawn_kernel_task(bringup_kernel_runner, 0);
        info!("[{LOG_TAG}] kernel runner enqueued ({} test(s) × {} libc)",
              BASIC_TESTS.len(),
              crate::user_bringup_common::LIBC_PREFIXES.len());
    }
    info!("[bringup][stage-basic] END");
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
extern "C" fn bringup_kernel_runner(_arg : usize) -> ! {
    #[cfg(feature = "vfs-bridge")]
    warn_missing_basic_assets();
    for prefix in crate::user_bringup_common::LIBC_PREFIXES {
        for test in BASIC_TESTS {
            crate::user_bringup_common::run_one_basic_elf(LOG_TAG, prefix, test);
        }
    }
    info!("[{LOG_TAG}] all tests finished");
    task::exit_current(0);
}

/// 启动期检查 oscomp basic 测例依赖的根卷路径。
#[cfg(all(any(feature = "impl-sv39", feature = "impl-loongarch64"),
          feature = "vfs-bridge"))]
fn warn_missing_basic_assets() {
    use vfs::api::SingleRootReadView;
    let view = vfs::root::read_view();
    for path in ["/glibc/basic/text.txt",
                 "/glibc/basic/mnt",
                 "/musl/basic/text.txt",
                 "/musl/basic/mnt"]
    {
        match view.exists(path) {
            Ok(true) => info!("[{LOG_TAG}] rootfs asset present: {path}"),
            Ok(false) => {
                warn!("[{LOG_TAG}] rootfs asset MISSING: {path} (oscomp fstat/openat may fail)")
            }
            Err(e) => warn!("[{LOG_TAG}] rootfs check {path}: {e:?}"),
        }
    }
}
