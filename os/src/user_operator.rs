//! 由构建期 feature 选择的用户态监督器，供现场操作员 profile 使用。

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use runtime::logging::{error, info, warn};

const LOG_TAG : &str = "operator";
static CONSOLE_INPUT_TASK_STARTED : AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum OperatorMode {
    /// 自动执行预设用户任务。
    Auto,
    /// 启动交互式 shell。
    Shell,
    /// 执行指定的一次性命令。
    Run,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum ExitPolicy {
    Shutdown,
    Shell,
    Reboot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum TtyMode {
    Interactive,
    Closed,
    Fixture,
}

#[derive(Debug)]
struct BootPlan {
    mode : OperatorMode,
    shell : Option<&'static str>,
    script : Option<&'static str>,
    on_exit : ExitPolicy,
    tty : TtyMode,
}

impl BootPlan {
    fn defaults() -> Self {
        Self { mode : OperatorMode::Auto,
               shell : None,
               script : None,
               on_exit : ExitPolicy::Shutdown,
               #[cfg(feature = "pre")]
               tty : TtyMode::Fixture,
               #[cfg(feature = "final_online")]
               tty : TtyMode::Closed }
    }

    fn selected_mode() -> OperatorMode {
        #[cfg(feature = "operator-shell")]
        {
            OperatorMode::Shell
        }
        #[cfg(feature = "operator-run")]
        {
            OperatorMode::Run
        }
        #[cfg(not(any(feature = "operator-shell", feature = "operator-run")))]
        {
            OperatorMode::Auto
        }
    }
}

#[cfg(all(feature = "operator-shell", feature = "operator-run"))]
compile_error!("features `operator-shell` and `operator-run` are mutually exclusive");

/// 从编译期 feature 与构建环境构造启动方案，不再读取 QEMU bootargs。
fn build_plan() -> BootPlan {
    let mut plan = BootPlan::defaults();
    match BootPlan::selected_mode() {
        OperatorMode::Auto => {}
        OperatorMode::Shell => {
            plan.mode = OperatorMode::Shell;
            plan.on_exit = ExitPolicy::Shell;
            plan.tty = TtyMode::Interactive;
            plan.shell = option_env!("WATEROS_OPERATOR_SHELL");
        }
        OperatorMode::Run => {
            plan.mode = OperatorMode::Run;
            plan.on_exit = ExitPolicy::Shutdown;
            plan.tty = TtyMode::Interactive;
            plan.script = option_env!("WATEROS_OPERATOR_SCRIPT");
        }
    }
    plan
}

struct ShellCandidate {
    program : String,
    argv0 : &'static str,
}

fn shell_candidates(requested : Option<&str>) -> Vec<ShellCandidate> {
    let mut result = Vec::new();
    if let Some(path) = requested {
        let argv0 = if path.ends_with("busybox") {
            "sh"
        } else if path.ends_with("bash") {
            "bash"
        } else {
            "sh"
        };
        result.push(ShellCandidate { program : path.to_string(),
                                     argv0 });
    }
    for (program, argv0) in [("/bin/bash", "bash"),
                             ("/bin/sh", "sh"),
                             ("/glibc/busybox", "sh"),
                             ("/musl/busybox", "sh")]
    {
        if !result.iter()
                  .any(|candidate| candidate.program == program)
        {
            result.push(ShellCandidate { program : program.to_string(),
                                         argv0 });
        }
    }
    result
}

fn shell_environment(shell : &str) -> Vec<String> {
    vec!["PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/\
          glibc"
                .to_string(),
         "HOME=/root".to_string(),
         "USER=root".to_string(),
         "LOGNAME=root".to_string(),
         "TERM=vt100".to_string(),
         "LANG=C".to_string(),
         "PWD=/root".to_string(),
         format!("SHELL={shell}"),]
}

fn run_shell_once(requested : Option<&str>, script : Option<&str>) -> bool {
    for candidate in shell_candidates(requested) {
        let env = shell_environment(&candidate.program);
        let env_refs : Vec<&str> = env.iter()
                                      .map(String::as_str)
                                      .collect();
        let mut argv = vec![candidate.argv0];
        if let Some(script) = script {
            argv.push(script);
        } else {
            // operator shell 必须显式进入交互模式。只传 `sh` 时，BusyBox/bash
            // 会根据启动时的 fd/进程组状态自行判断是否交互；SMP 下这段探测可能
            // 发生在控制终端状态发布完成之前，结果是 shell 阻塞读取但不显示提示符。
            argv.push("-i");
        }
        info!("[{LOG_TAG}] trying shell={} argv={argv:?}",
              candidate.program);
        if let Some(code) =
            crate::user_bringup_common::run_one_elf_argv_env_exit(LOG_TAG,
                                                                  &candidate.program,
                                                                  &argv,
                                                                  &env_refs)
        {
            warn!("[{LOG_TAG}] shell={} exited code={code}",
                  candidate.program);
            return true;
        }
    }
    error!("[{LOG_TAG}] no usable shell found; attach GDB or replace the root image");
    false
}

pub(crate) fn start() { task::spawn_kernel_task(operator_main, 0); }

fn configure_tty(mode : TtyMode) {
    let mode = match mode {
        TtyMode::Interactive => tty::ConsoleTtyMode::Interactive,
        TtyMode::Closed => tty::ConsoleTtyMode::Closed,
        TtyMode::Fixture => tty::ConsoleTtyMode::Fixture,
    };
    tty::configure(mode);
    if mode == tty::ConsoleTtyMode::Interactive {
        start_console_input_task();
    }
}

fn start_console_input_task() {
    if !CONSOLE_INPUT_TASK_STARTED.swap(true, Ordering::AcqRel) {
        task::spawn_kernel_task(console_input_main, 0);
    }
}

extern "C" fn console_input_main(_arg : usize) -> ! {
    loop {
        if let Some(event) = vfs::fd::poll_console_input_once() {
            let _ = syscall::send_kernel_signal_to_process_group(event.process_group, event.signal);
        } else {
            platform::arch::interrupt::wait_for_interrupt();
            task::yield_now();
        }
    }
}

extern "C" fn operator_main(_arg : usize) -> ! {
    let plan = build_plan();
    configure_tty(plan.tty);
    info!("[{LOG_TAG}] plan={plan:?}");

    match plan.mode {
        OperatorMode::Auto => crate::user_bringup_busybox::run_auto_queue(0),
        OperatorMode::Run => {
            let _ = run_shell_once(plan.shell
                                       .as_deref(),
                                   plan.script
                                       .as_deref());
        }
        OperatorMode::Shell => {
            let _ = run_shell_once(plan.shell
                                       .as_deref(),
                                   None);
        }
    }

    match plan.on_exit {
        ExitPolicy::Shutdown => {
            let _ = platform::reset::shutdown(platform::reset::PlatformResetReason::NoReason);
        }
        ExitPolicy::Reboot => {
            let _ = platform::reset::reboot(platform::reset::PlatformResetReason::NoReason);
        }
        ExitPolicy::Shell => {}
    }

    loop {
        if !run_shell_once(plan.shell
                               .as_deref(),
                           None)
        {
            task::sleep_for_ticks(100);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_build_selects_automatic_evaluation() {
        #[cfg(not(any(feature = "operator-shell", feature = "operator-run")))]
        {
            let plan = build_plan();
            assert_eq!(plan.mode, OperatorMode::Auto);
            assert_eq!(plan.on_exit, ExitPolicy::Shutdown);
        }
        #[cfg(feature = "operator-shell")]
        assert_eq!(build_plan().mode, OperatorMode::Shell);
        #[cfg(feature = "operator-run")]
        assert_eq!(build_plan().mode, OperatorMode::Run);
    }

    #[test]
    fn requested_shell_precedes_stable_fallbacks_without_duplicates() {
        let candidates = shell_candidates(Some("/musl/busybox"));
        let paths : Vec<&str> = candidates.iter()
                                          .map(|candidate| {
                                              candidate.program
                                                       .as_str()
                                          })
                                          .collect();
        assert_eq!(paths, ["/musl/busybox",
                           "/bin/bash",
                           "/bin/sh",
                           "/glibc/busybox"]);
    }
}
