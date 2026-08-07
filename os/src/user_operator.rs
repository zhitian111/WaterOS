//! Runtime-selectable user supervisor used by the on-site operator profile.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use runtime::logging::{error, info, warn, LevelFilter};

const LOG_TAG : &str = "operator";
static CONSOLE_INPUT_TASK_STARTED : AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperatorMode {
    Auto,
    Shell,
    Run,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExitPolicy {
    Shutdown,
    Shell,
    Reboot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TtyMode {
    Interactive,
    Closed,
    Fixture,
}

#[derive(Debug)]
struct BootPlan {
    mode : OperatorMode,
    shell : Option<String>,
    script : Option<String>,
    on_exit : ExitPolicy,
    tty : TtyMode,
    log : Option<LevelFilter>,
    invalid : bool,
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
               tty : TtyMode::Closed,
               log : None,
               invalid : false }
    }

    fn parse(command_line : Option<&str>) -> Self {
        let mut plan = Self::defaults();
        let mut explicit_exit = false;
        let mut explicit_tty = false;
        for field in command_line.unwrap_or("")
                                 .split_ascii_whitespace()
        {
            let Some((key, value)) = field.split_once('=') else {
                continue;
            };
            match key {
                "wos.mode" => match value {
                    "auto" => plan.mode = OperatorMode::Auto,
                    "shell" => plan.mode = OperatorMode::Shell,
                    "run" => plan.mode = OperatorMode::Run,
                    _ => plan.invalid = true,
                },
                "wos.shell" if value.starts_with('/') => plan.shell = Some(value.to_string()),
                "wos.shell" => plan.invalid = true,
                "wos.script" if value.starts_with('/') => plan.script = Some(value.to_string()),
                "wos.script" => plan.invalid = true,
                "wos.on_exit" => {
                    explicit_exit = true;
                    match value {
                        "shutdown" => plan.on_exit = ExitPolicy::Shutdown,
                        "shell" => plan.on_exit = ExitPolicy::Shell,
                        "reboot" => plan.on_exit = ExitPolicy::Reboot,
                        _ => plan.invalid = true,
                    }
                }
                "wos.tty" => {
                    explicit_tty = true;
                    match value {
                        "interactive" => plan.tty = TtyMode::Interactive,
                        "closed" => plan.tty = TtyMode::Closed,
                        "fixture" => plan.tty = TtyMode::Fixture,
                        _ => plan.invalid = true,
                    }
                }
                "wos.log" => {
                    plan.log = match value {
                        "error" => Some(LevelFilter::Error),
                        "warn" => Some(LevelFilter::Warn),
                        "info" => Some(LevelFilter::Info),
                        "debug" => Some(LevelFilter::Debug),
                        "trace" => Some(LevelFilter::Trace),
                        _ => {
                            plan.invalid = true;
                            None
                        }
                    }
                }
                // Internal QEMU topology hint consumed by the LoongArch
                // platform SMP backend. Validate it here so a malformed run
                // command follows the same rescue policy as other known
                // WaterOS options.
                "wos.cpus" => {
                    if value.parse::<usize>()
                            .ok()
                            .filter(|count| (1..=base_config::task::MAX_CPUS).contains(count))
                            .is_none()
                    {
                        plan.invalid = true;
                    }
                }
                key if key.starts_with("wos.") => {
                    warn!("[{LOG_TAG}] ignored unknown boot option {key}")
                }
                _ => {}
            }
        }
        if plan.mode != OperatorMode::Auto {
            if !explicit_exit {
                plan.on_exit = ExitPolicy::Shell;
            }
            if !explicit_tty {
                plan.tty = TtyMode::Interactive;
            }
            if plan.log.is_none() {
                plan.log = Some(LevelFilter::Warn);
            }
        }
        if plan.mode == OperatorMode::Run &&
           plan.script
               .is_none()
        {
            plan.invalid = true;
        }
        if plan.invalid {
            plan.mode = OperatorMode::Shell;
            plan.on_exit = ExitPolicy::Shell;
            plan.tty = TtyMode::Interactive;
            plan.log
                .get_or_insert(LevelFilter::Warn);
        }
        plan
    }
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

/// 根据平台提供的可选命令行生成启动方案。
///
/// COMPETITION_BOOT: 决赛评测的 QEMU 命令不包含 `-append`，因此缺少
/// bootargs 是正常启动方式，必须立即采用由 `pre` / `final_online` feature
/// 决定的自动评测默认值，不能等待串口菜单。开发环境仍可通过 `wos.*`
/// 参数覆盖为 shell 或指定脚本模式。
fn plan_from_command_line(command_line : Option<&str>) -> BootPlan {
    match command_line {
        Some(command_line) => BootPlan::parse(Some(command_line)),
        None => BootPlan::defaults(),
    }
}

extern "C" fn operator_main(_arg : usize) -> ! {
    let command_line = platform::boot::command_line();
    let plan = plan_from_command_line(command_line);
    if plan.invalid {
        error!("[{LOG_TAG}] invalid WaterOS boot options: {:?}; entering rescue shell",
               command_line);
    }
    if let Some(level) = plan.log {
        runtime::logging::set_max_level(level);
    }
    configure_tty(plan.tty);
    info!("[{LOG_TAG}] command_line={command_line:?} plan={plan:?}");

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
    fn missing_bootargs_start_automatic_evaluation_immediately() {
        let plan = plan_from_command_line(None);
        assert_eq!(plan.mode, OperatorMode::Auto);
        assert_eq!(plan.on_exit, ExitPolicy::Shutdown);
        assert!(!plan.invalid);
        #[cfg(feature = "pre")]
        assert_eq!(plan.tty, TtyMode::Fixture);
        #[cfg(feature = "final_online")]
        assert_eq!(plan.tty, TtyMode::Closed);
    }

    #[test]
    fn operator_defaults_to_interactive_rescue() {
        let plan = BootPlan::parse(Some("wos.mode=shell"));
        assert_eq!(plan.mode, OperatorMode::Shell);
        assert_eq!(plan.on_exit, ExitPolicy::Shell);
        assert_eq!(plan.tty, TtyMode::Interactive);
    }

    #[test]
    fn invalid_known_option_forces_rescue_shell() {
        let plan = BootPlan::parse(Some("wos.mode=broken"));
        assert!(plan.invalid);
        assert_eq!(plan.mode, OperatorMode::Shell);
    }

    #[test]
    fn run_requires_an_absolute_script() {
        assert!(BootPlan::parse(Some("wos.mode=run")).invalid);
        assert!(BootPlan::parse(Some("wos.mode=run wos.script=relative.sh")).invalid);
        let plan = BootPlan::parse(Some("wos.mode=run wos.script=/root/test.sh"));
        assert!(!plan.invalid);
        assert_eq!(plan.mode, OperatorMode::Run);
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
