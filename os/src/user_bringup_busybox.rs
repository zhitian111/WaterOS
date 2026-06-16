//! `stage-busybox`：挂载根卷后登记内核 runner，串行执行根卷内带 shebang 的测试脚本。

use runtime::logging::*;

/// 测试脚本完整路径（根卷内）；推荐顺序见 `docs/roadmap/test-case-full-pass-plan.md`。
///
/// **用法**：每次只取消注释**一个阶段**的脚本，再 `make rv_qemu_run`。
/// 未实现的 syscall 会直接 panic，切勿一次启用多阶段或全部脚本。
///
/// 阶段划分：
/// - **P1 basic**：syscall 基础测例（31/32，mount 待修）
/// - **P2 busybox + lua**：busybox 命令表 + lua 脚本
/// - **P3 benchmark**：lmbench / unixbench / libcbench / iozone
/// - **P4 网络**：iperf / netperf（`rv_qemu_run.sh` 需启用 virtio-net）
/// - **P5 libctest + cyclictest**
/// - **P6 LTP**：用例量最大，单独跑
const SCRIPT_PATHS : &[&str] = &[
                                 // --- P1 basic ---
                                 //"/glibc/basic_testcode.sh",        // done
                                 // "/musl/basic_testcode.sh",         // done
                                 // --- P2 busybox + lua ---
                                 // "/glibc/busybox_testcode.sh",      // done
                                 // "/musl/busybox_testcode.sh",       // done
                                 // "/glibc/lua_testcode.sh",          // done
                                 // "/musl/lua_testcode.sh",           // done
                                 // --- P3 benchmark ---
                                 // "/glibc/lmbench_testcode.sh",
                                 // "/musl/lmbench_testcode.sh",
                                 //"/glibc/unixbench_testcode.sh",
                                 // "/musl/unixbench_testcode.sh",
                                 //"/glibc/libcbench_testcode.sh",
                                 // "/musl/libcbench_testcode.sh",
                                  "/glibc/iozone_testcode.sh",
                                 // "/musl/iozone_testcode.sh",
                                 // --- P4 网络（需 rv_qemu_run.sh 启用 virtio-net）---
                                 // "/glibc/iperf_testcode.sh",        // done
                                 // "/musl/iperf_testcode.sh",
                                 // "/glibc/netperf_testcode.sh",      // done
                                 // "/musl/netperf_testcode.sh",
                                 // --- P5 libctest + cyclictest ---
                                 // "/glibc/libctest_testcode.sh",     // done
                                 // "/musl/libctest_testcode.sh",      // done
                                 //"/glibc/cyclictest_testcode.sh",
                                 //"/musl/cyclictest_testcode.sh",
                                 // --- P6 LTP ---
                                 // "/glibc/ltp_testcode.sh",
                                 // "/musl/ltp_testcode.sh",
];

const LOG_TAG : &str = "busybox-bringup";

/// 执行 `stage-busybox`：登记内核串行 runner（不阻塞；用户态在 `run_first_task` 后运行）。
pub fn run_stage_busybox() {
    info!("[bringup][stage-busybox] BEGIN");
    task::spawn_kernel_task(bringup_kernel_runner, 0);
    info!("[{LOG_TAG}] kernel runner enqueued ({} script(s))",
          SCRIPT_PATHS.len());
    info!("[bringup][stage-busybox] END");
}

extern "C" fn bringup_kernel_runner(_arg : usize) -> ! {
    use platform::reset::shutdown;

    for script_path in SCRIPT_PATHS {
        info!("[{LOG_TAG}] script_path = {script_path}");
        crate::user_bringup_common::run_one_busybox_script(LOG_TAG, script_path);
    }
    info!("[{LOG_TAG}] all scripts finished");
    let _ = shutdown(platform::reset::PlatformResetReason::NoReason);
    task::exit_current(0);
}
