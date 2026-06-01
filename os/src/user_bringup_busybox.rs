//! `stage-busybox`：挂载根卷后登记内核 runner，串行执行
//! `/{glibc,musl}/busybox sh <脚本完整路径>`。
//!
//! 脚本路径写在 [`SCRIPT_PATHS`] 中，不需要的项注释掉即可。

use runtime::logging::*;

/// 测试脚本完整路径（根卷内）；推荐顺序见 `docs/roadmap/test-case-full-pass-plan.md`。
const SCRIPT_PATHS : &[&str] = &[
    "/glibc/basic_testcode.sh",
    // "/musl/basic_testcode.sh",
    // "/glibc/busybox_testcode.sh",
    // "/musl/busybox_testcode.sh",
    // "/glibc/lua_testcode.sh",
    // "/musl/lua_testcode.sh",
    // "/glibc/libctest_testcode.sh",
    // "/musl/libctest_testcode.sh",
    // "/glibc/iozone_testcode.sh",
    // "/musl/iozone_testcode.sh",
    // "/glibc/unixbench_testcode.sh",
    // "/musl/unixbench_testcode.sh",
    // "/glibc/lmbench_testcode.sh",
    // "/musl/lmbench_testcode.sh",
    // "/glibc/iperf_testcode.sh",
    // "/musl/iperf_testcode.sh",
    // "/glibc/netperf_testcode.sh",
    // "/musl/netperf_testcode.sh",
    // "/glibc/libcbench_testcode.sh",
    // "/musl/libcbench_testcode.sh",
    // "/glibc/cyclictest_testcode.sh",
    // "/musl/cyclictest_testcode.sh",
    // "/glibc/ltp_testcode.sh",
    // "/musl/ltp_testcode.sh",
];

const LOG_TAG : &str = "busybox-bringup";
const GLIBC_BUSYBOX_PATH : &str = "/glibc/busybox";
const GLIBC_BASIC_SCRIPT : &str = "/glibc/basic_testcode.sh";
const GLIBC_BASIC_INLINE : &str = "echo \"#### OS COMP TEST GROUP START basic ####\"; \
                                   cd ./basic; \
                                   for i in brk chdir clone close dup2 dup execve exit fork fstat \
                                   getcwd getdents getpid getppid gettimeofday mkdir_ mmap mount \
                                   munmap openat open pipe read sleep times umount uname unlink \
                                   wait waitpid write yield; do echo \"Testing $i :\"; ./$i; done; \
                                   cd ..; echo \"#### OS COMP TEST GROUP END basic ####\"";

/// 执行 `stage-busybox`：登记内核串行 runner（不阻塞；用户态在 `run_first_task` 后运行）。
pub fn run_stage_busybox() {
    info!("[bringup][stage-busybox] BEGIN");
    #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
    {
        warn!("[{LOG_TAG}] no mm impl: skip");
        info!("[bringup][stage-busybox] END");
        return;
    }
    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    {
        task::spawn_kernel_task(bringup_kernel_runner, 0);
        info!("[{LOG_TAG}] kernel runner enqueued ({} script(s))",
              SCRIPT_PATHS.len());
    }
    info!("[bringup][stage-busybox] END");
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
extern "C" fn bringup_kernel_runner(_arg : usize) -> ! {
    if !run_busybox_probes() {
        warn!("[{LOG_TAG}] probes failed; skip script table");
        task::exit_current(0);
    }
    for script_path in SCRIPT_PATHS {
        run_script_or_inline(script_path);
    }
    info!("[{LOG_TAG}] all scripts finished");
    task::exit_current(0);
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
fn run_script_or_inline(script_path : &str) {
    if script_path == GLIBC_BASIC_SCRIPT {
        // The current oscomp image carries a zero-filled `/glibc/basic_testcode.sh`
        // wrapper, while `/glibc/basic/run-all.sh` has the real test list.
        // Run the equivalent shell command inline so BusyBox can reach basic.
        let argv = [GLIBC_BUSYBOX_PATH, "sh", "-c", GLIBC_BASIC_INLINE];
        info!("[{LOG_TAG}] SCRIPT INLINE path={script_path} argv={argv:?}");
        let _ = crate::user_bringup_common::run_one_busybox_argv(LOG_TAG,
                                                                 GLIBC_BUSYBOX_PATH,
                                                                 &argv);
        return;
    }
    crate::user_bringup_common::run_one_busybox_script(LOG_TAG, script_path);
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
fn run_busybox_probes() -> bool {
    let echo_argv = [GLIBC_BUSYBOX_PATH, "echo", "__busybox_echo_ok__"];
    if !run_probe("echo", &echo_argv) {
        return false;
    }

    let sh_c_argv = [GLIBC_BUSYBOX_PATH, "sh", "-c", "echo __busybox_sh_c_ok__"];
    if !run_probe("sh-c", &sh_c_argv) {
        return false;
    }

    true
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
fn run_probe(name : &str, argv : &[&str]) -> bool {
    info!("[{LOG_TAG}] PROBE START name={name} argv={argv:?}");
    let exit_code = crate::user_bringup_common::run_one_busybox_argv(LOG_TAG,
                                                                     GLIBC_BUSYBOX_PATH,
                                                                     argv);
    match exit_code {
        Some(0) => {
            info!("[{LOG_TAG}] PROBE END name={name} exit_code=0");
            true
        }
        Some(code) => {
            warn!("[{LOG_TAG}] PROBE FAIL name={name} exit_code={code}");
            false
        }
        None => {
            warn!("[{LOG_TAG}] PROBE FAIL name={name} not-run");
            false
        }
    }
}
