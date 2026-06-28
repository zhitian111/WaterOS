//! `stage-busybox`：挂载根卷后登记内核 runner，串行执行根卷内带 shebang 的测试脚本。

use runtime::logging::*;

use crate::user_bringup_common::BringupCommand;

#[cfg(feature = "bringup-ltp-glibc-only")]
const BRINGUP_COMMANDS : &[BringupCommand] = &[BringupCommand { program : "/glibc/busybox",
                                                                argv:
                                                                    &["timeout",
                                                                      "7200",
                                                                      "sh",
                                                                      "/glibc/ltp_testcode.sh"] }];

#[cfg(feature = "bringup-ltp-musl-only")]
const BRINGUP_COMMANDS : &[BringupCommand] = &[BringupCommand { program : "/musl/busybox",
                                                                argv : &["timeout",
                                                                         "7200",
                                                                         "sh",
                                                                         "/musl/ltp_testcode.\
                                                                          sh"] }];

#[cfg(all(not(feature = "bringup-ltp-glibc-only"),
          not(feature = "bringup-ltp-musl-only")))]
// iozone 及之后各脚本 wall timeout（秒），便于分配总时间；评测机 QEMU 另有 3600s 总限。
// iozone 900 | lmbench 600 | libcbench 600 | unixbench 1800 | ltp 7200
const BRINGUP_COMMANDS : &[BringupCommand] = &[BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/basic_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/basic_testcode.sh"] }, // done
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/busybox_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/busybox_testcode.sh"] }, // done
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/lua_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/lua_testcode.sh"] }, // done
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/iperf_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/iperf_testcode.sh"] }, // done
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/netperf_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/netperf_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/libctest_testcode.sh"] }, // done
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/cyclictest_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/cyclictest_testcode.sh"] }, // done
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/iozone_testcode.sh"] },
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/iozone_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox",
        argv : &["timeout",
            "2700",
            "sh",
            "/glibc/ltp_testcode.sh"] },
    BringupCommand { program : "/musl/busybox",
        argv : &["timeout",
            "2700",
            "sh",
            "/musl/ltp_testcode.sh"] },
    BringupCommand { program : "/glibc/busybox",
        argv : &["timeout",
            "600",
            "sh",
            "/glibc/libcbench_testcode.sh"] }, // done
    BringupCommand { program : "/musl/busybox",
        argv : &["timeout",
            "600",
            "sh",
            "/musl/libcbench_testcode.sh"] }, // done
      BringupCommand { program : "/glibc/busybox",
                       argv : &["timeout",
                                "600",
                                "sh",
                                "/glibc/lmbench_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["timeout",
                                "600",
                                "sh",
                                "/musl/lmbench_testcode.sh"] }, // done
      BringupCommand { program : "/glibc/busybox",
                       argv : &["timeout",
                                "1800",
                                "sh",
                                "/glibc/unixbench_testcode.sh"] }, // done
      BringupCommand { program : "/musl/busybox",
                       argv : &["timeout",
                                "1800",
                                "sh",
                                "/musl/unixbench_testcode.sh"] }, // done
];

const LOG_TAG : &str = "busybox-bringup";

fn monotonic_ns() -> u128 {
    platform::timer::now_duration().map(|duration| duration.as_nanos())
                                   .unwrap_or(0)
}

fn log_elapsed(log_tag : &str, cmd : &BringupCommand, start_ns : u128, end_ns : u128) {
    let elapsed_ns = end_ns.saturating_sub(start_ns);
    let sec = elapsed_ns / 1_000_000_000;
    let ms = (elapsed_ns % 1_000_000_000) / 1_000_000;
    warn!("[{log_tag}] program={} argv={:?} end_mono_ns={end_ns} elapsed={sec}s {ms}ms \
           ({elapsed_ns}ns)",
          cmd.program, cmd.argv);
}

/// 执行 `stage-busybox`：登记内核串行 runner（不阻塞；用户态在 `run_first_task` 后运行）。
pub fn run_stage_busybox() {
    warn!("[bringup][stage-busybox] BEGIN");
    task::spawn_kernel_task(bringup_kernel_runner, 0);
    warn!("[{LOG_TAG}] kernel runner enqueued ({} command(s))",
          BRINGUP_COMMANDS.len());
    warn!("[bringup][stage-busybox] END");
}

extern "C" fn bringup_kernel_runner(_arg : usize) -> ! {
    use platform::reset::shutdown;

    crate::user_bringup_root_layout::refresh_ltp_accounts();

    for cmd in BRINGUP_COMMANDS {
        let start_ns = monotonic_ns();
        warn!("[{LOG_TAG}] program={} argv={:?} start_mono_ns={start_ns}",
              cmd.program, cmd.argv);
        crate::user_bringup_common::run_one_bringup_command(LOG_TAG, cmd);
        log_elapsed(LOG_TAG, cmd, start_ns, monotonic_ns());
    }
    warn!("[{LOG_TAG}] all commands finished");
    let _ = shutdown(platform::reset::PlatformResetReason::NoReason);
    task::exit_current(0);
}
