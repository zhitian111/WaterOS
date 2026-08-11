//! `stage-busybox`: mount the root volume, then run the image-appropriate test queue.

use runtime::logging::*;
use vfs::api::SingleRootReadView;

use crate::user_bringup_common::BringupCommand;

/// Non-`pub` images carry the preliminary competition scripts.
const PRELIMINARY_COMMANDS : &[BringupCommand] =
    &[BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/cyclictest_testcode.sh"] },
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/cyclictest_testcode.sh"] },
      BringupCommand { program : "/musl/busybox",
                       argv : &["sh",
                                "/musl/ltp_testcode.sh"] },
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
                                "/glibc/ltp_testcode.sh"] },
      BringupCommand { program : "/glibc/busybox",
                       argv : &["sh",
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
                                "/musl/iozone_testcode.sh"] }];

/// `pub` images carry these two final-round scripts.
const FINAL_COMMANDS : &[BringupCommand] =
    &[BringupCommand { program : "/glibc/cagent_testcode.sh",
                       argv : &["/glibc/cagent_testcode.sh"] },
      BringupCommand { program : "/glibc/buildstorm_testcode.sh",
                       argv : &["/glibc/buildstorm_testcode.sh"] }];

/// This script is present only in the organizer-provided `pub` image, so it is a stable image
/// format marker rather than a build-time policy switch.
const FINAL_IMAGE_MARKER : &str = "/glibc/cagent_testcode.sh";
const LOG_TAG : &str = "busybox-bringup";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BringupImage {
    Preliminary,
    Final,
}

fn detect_bringup_image() -> Option<BringupImage> {
    match vfs::root::read_view().exists(FINAL_IMAGE_MARKER) {
        Ok(true) => Some(BringupImage::Final),
        Ok(false) => Some(BringupImage::Preliminary),
        Err(error) => {
            error!("[{LOG_TAG}] cannot probe final-image marker {FINAL_IMAGE_MARKER}: {error:?}");
            None
        }
    }
}

fn commands_for_image(image : BringupImage) -> &'static [BringupCommand] {
    match image {
        BringupImage::Preliminary => PRELIMINARY_COMMANDS,
        BringupImage::Final => FINAL_COMMANDS,
    }
}

/// Keep the libc search path coupled to the detected image format.  This avoids the generic
/// loader's build-feature heuristic selecting the final-round environment for an LTP image.
fn envp_for_command(image : BringupImage, program : &str) -> &'static [&'static str] {
    match image {
        BringupImage::Final => &["PATH=/glibc:/bin:/usr/bin:/sbin:/usr/sbin"],
        BringupImage::Preliminary if program.starts_with("/glibc/") => {
            &["LD_LIBRARY_PATH=/glibc/lib",
              "PATH=/glibc/ltp/testcases/bin:/glibc/ltp/testcases/lib:/glibc:/bin:/usr/bin:/sbin:/\
               usr/sbin"]
        }
        BringupImage::Preliminary if program.starts_with("/musl/") => {
            &["LD_LIBRARY_PATH=/musl/lib",
              "PATH=/musl/ltp/testcases/bin:/musl/ltp/testcases/lib:/musl:/bin:/usr/bin:/sbin:/\
               usr/sbin"]
        }
        BringupImage::Preliminary => &[],
    }
}

/// Monotonic clock in nanoseconds (zero if unavailable).
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

/// Enqueue the kernel runner; image detection happens after the root filesystem is available.
pub fn run_stage_busybox() {
    error!("[bringup][stage-busybox] BEGIN");
    crate::user_operator::start();
    error!("[{LOG_TAG}] kernel runner enqueued; queue will be selected from root image marker");
    error!("[bringup][stage-busybox] END");
}

pub(crate) extern "C" fn run_auto_queue(_arg : usize) -> ! {
    use platform::reset::shutdown;

    info!("entered runner");
    let Some(image) = detect_bringup_image() else {
        error!("[{LOG_TAG}] no bring-up queue selected; stop runner");
        let _ = shutdown(platform::reset::PlatformResetReason::NoReason);
        task::exit_current(1);
    };
    let commands = commands_for_image(image);
    error!("[{LOG_TAG}] detected {image:?} image; running {} command(s)",
           commands.len());

    if image == BringupImage::Preliminary {
        crate::user_bringup_root_layout::prune_ltp_excluded_testcases();
        crate::user_bringup_root_layout::refresh_ltp_accounts();
    }

    for cmd in commands {
        let start_ns = monotonic_ns();
        error!("[{LOG_TAG}] program={} argv={:?} start_mono_ns={start_ns}",
               cmd.program, cmd.argv);
        let exit_code =
            crate::user_bringup_common::run_one_elf_argv_env_exit(LOG_TAG,
                                                                  cmd.program,
                                                                  cmd.argv,
                                                                  envp_for_command(image,
                                                                                   cmd.program));
        log_elapsed(LOG_TAG, cmd, start_ns, monotonic_ns());
        match exit_code {
            Some(0) => error!("[{LOG_TAG}] command succeeded program={} exit_code=0",
                              cmd.program),
            Some(code) => {
                error!("[{LOG_TAG}] command failed program={} exit_code={code}; stop queue",
                       cmd.program);
                break;
            }
            None => {
                error!("[{LOG_TAG}] command failed to load or spawn program={}; stop queue",
                       cmd.program);
                break;
            }
        }
    }
    error!("[{LOG_TAG}] all commands finished");
    let _ = shutdown(platform::reset::PlatformResetReason::NoReason);
    task::exit_current(0);
}
