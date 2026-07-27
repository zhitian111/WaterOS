//! `stage-busybox`：挂载根卷后登记内核 runner，串行执行根卷内带 shebang 的测试脚本。

use runtime::logging::*;

use crate::user_bringup_common::BringupCommand;

#[cfg(all(feature = "pre", feature = "bringup-ltp-glibc-only"))]
/// 仅跑 glibc LTP 长测（feature `bringup-ltp-glibc-only`）。
const BRINGUP_COMMANDS : &[BringupCommand] =
    &[BringupCommand { program : "/glibc/busybox",
                       argv : &["timeout",
                                "300",
                                "sh",
                                "-c",
                                "cd /glibc && echo '#### OS COMP TEST GROUP START \
                                 lmbench-glibc ####' && echo context switch overhead && \
                                 ./lmbench_all lat_ctx -P 1 -s 32 2 4 8 16 24 32 64 96 && \
                                 echo '#### OS COMP TEST GROUP END lmbench-glibc ####'"] }];

#[cfg(all(feature = "pre", feature = "bringup-ltp-musl-only"))]
/// 仅跑 musl LTP 长测（feature `bringup-ltp-musl-only`）。
const BRINGUP_COMMANDS : &[BringupCommand] = &[BringupCommand { program : "/musl/busybox",
                                                                argv : &["timeout",
                                                                         "7200",
                                                                         "sh",
                                                                         "/musl/ltp_testcode.\
                                                                          sh"] }];

#[cfg(all(feature = "pre",
          not(feature = "bringup-ltp-glibc-only"),
          not(feature = "bringup-ltp-musl-only")))]
// 默认赛题脚本队列：目标 wall ~35–45 min（timeout 上限 ~60 min）；LTP 放最后。
// iozone 180 | libcbench 180 | lmbench 360 | unixbench 360 | ltp 480
const BRINGUP_COMMANDS : &[BringupCommand] = &[
    BringupCommand { program : "/glibc/busybox",
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
      BringupCommand { program : "/musl/busybox",
                       argv : &[
                                "sh",
                                "/musl/ltp_testcode.sh"] },
      BringupCommand { program : "/glibc/busybox",
                       argv : &[
                                "sh",
                                "/glibc/ltp_testcode.sh"] },
      BringupCommand { program : "/glibc/busybox",
                       argv : &[
            "sh",
                                "/glibc/libcbench_testcode.sh"] },
    BringupCommand { program : "/musl/busybox",
        argv : &["sh",
            "/musl/libcbench_testcode.sh"] },
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/lmbench_testcode.sh"] },
    BringupCommand { program : "/musl/busybox",
        argv : &["sh",
            "/musl/lmbench_testcode.sh"] },
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/iozone_testcode.sh"] },
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/iozone_testcode.sh"] },
      // unixbench 不是比赛测试的内容，已经弃用
      // BringupCommand { program : "/glibc/busybox",
      //                  argv : &["timeout",
      //                           "300",
      //                           "sh",
      //                           "/glibc/unixbench_testcode.sh"] },
      // BringupCommand { program : "/musl/busybox",
      //                  argv : &["timeout",
      //                           "600",
                                // "sh",
                                // "/musl/unixbench_testcode.sh"] }
    ];

/// 线上决赛镜像的两组 glibc 测例，路径与 `final_test_case/README.md` 一致。
#[cfg(all(feature = "final_online", feature = "final_online_smp_test"))]
const BRINGUP_COMMANDS : &[BringupCommand] =
    &[BringupCommand { program : "/busybox",
                       argv : &["sh",
                                "-c",
                                "rm -rf /tmp/wateros-smp-test; mkdir -p \
                                 /tmp/wateros-smp-test; i=0; while [ $i -lt 32 ]; do (echo \
                                 $i > /tmp/wateros-smp-test/$i; cat /tmp/wateros-smp-test/$i \
                                 > /dev/null; rm -f /tmp/wateros-smp-test/$i) & i=$((i + \
                                 1)); done; wait; echo SMP_MM_TEST_DONE"] },
      BringupCommand { program : "/busybox",
                       argv : &["sh",
                                "/glibc/cagent_testcode.sh"] },
      BringupCommand { program : "/busybox",
                       argv : &["sh",
                                "/glibc/buildstorm_testcode.sh"] }];

#[cfg(all(feature = "final_online", not(feature = "final_online_smp_test")))]
const BRINGUP_COMMANDS : &[BringupCommand] =
    &[BringupCommand { program : "/usr/bin/busybox",
                       argv : &["sh",
                                "/glibc/cagent_testcode.sh"] },
      BringupCommand { program : "/usr/bin/busybox",
                       argv : &["sh",
                                "/glibc/buildstorm_testcode.sh"] }];

const LOG_TAG : &str = "busybox-bringup";

/// 单调时钟纳秒（用于 bring-up 耗时统计；失败时返回 0）。
fn monotonic_ns() -> u128 {
    platform::timer::now_duration().map(|duration| duration.as_nanos())
                                   .unwrap_or(0)
}

fn log_elapsed(log_tag : &str, cmd : &BringupCommand, start_ns : u128, end_ns : u128) {
    let elapsed_ns = end_ns.saturating_sub(start_ns);
    let sec = elapsed_ns / 1_000_000_000;
    let ms = (elapsed_ns % 1_000_000_000) / 1_000_000;
    error!("[{log_tag}] program={} argv={:?} end_mono_ns={end_ns} elapsed={sec}s {ms}ms \
            ({elapsed_ns}ns)",
           cmd.program, cmd.argv);
}

/// 执行 `stage-busybox`：登记内核串行 runner（不阻塞；用户态在 `run_first_task` 后运行）。
pub fn run_stage_busybox() {
    error!("[bringup][stage-busybox] BEGIN");
    task::spawn_kernel_task(bringup_kernel_runner, 0);
    error!("[{LOG_TAG}] kernel runner enqueued ({} command(s))",
           BRINGUP_COMMANDS.len());
    error!("[bringup][stage-busybox] END");
}

extern "C" fn bringup_kernel_runner(_arg : usize) -> ! {
    use platform::reset::shutdown;
    info!("entered runner");
    #[cfg(feature = "pre")]
    crate::user_bringup_root_layout::refresh_ltp_accounts();

    for cmd in BRINGUP_COMMANDS {
        let start_ns = monotonic_ns();
        error!("[{LOG_TAG}] program={} argv={:?} start_mono_ns={start_ns}",
               cmd.program, cmd.argv);
        crate::user_bringup_common::run_one_bringup_command(LOG_TAG, cmd);
        log_elapsed(LOG_TAG, cmd, start_ns, monotonic_ns());
    }
    error!("[{LOG_TAG}] all commands finished");
    let _ = shutdown(platform::reset::PlatformResetReason::NoReason);
    task::exit_current(0);
}
