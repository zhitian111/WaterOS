//! 任务调度、等待/超时、退出与回收、以及用户态（含可选根卷
//! ELF）相关的内核自检。
//!
//! **用途**：在 `kernel_main` 已初始化 MM 与 `task`
//! 子系统后，通过多组内核任务与用户任务 验证 `sleep`/`yield`、`block`/`wake`、
//! 等待队列超时、`wait_for_exit`/`reap`、以及 `spawn_user_task_spec` 与 trap
//! 快照等契约。
//!
//! **入口**：[`spawn_all`] 由 `kernel_main` 调用；其中在 **`wateros` 根 crate 启用
//! `impl-sv39` feature**（`qemu-riscv64-opensbi` 已包含）时，会先尝试加载
//! [`mm::kernel_mm::DEFAULT_USER_ELF_PATH`] 对应的真实用户 ELF
//! 并派生观察任务，再创建 stage2/3/4
//! 内核自检任务与内核映射的用户态自检任务，
//! 以降低自检对默认用户程序首轮调度的干扰。
//!
//! **后续替换点**：自检任务与用户镜像地址/系统调用号为 QEMU
//! 调试路径上的固定约定，若 用户 ABI 或镜像布局变更，
//! 应同步调整本模块常量与断言。

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};
use mm::api::addr::VirtAddr;
use mm::api::perm::PagePerm;
use runtime::logging::*;

static BLOCK_TASK_READY : AtomicBool = AtomicBool::new(false);
// ---- stage4 内核映射用户任务：须与 `trap_handler` / `wateros-abi` 中 syscall 号及
// `spawn_user_task_spec` 断言一致（镜像范围仅用于元数据，代码实际在可执行内核 VA）。----
const STAGE4_USER_EXIT_OK : isize = 88;
const STAGE4_USER_EXIT_BAD_RET : isize = 89;
const STAGE4_USER_SYSCALL_YIELD_NR : usize = 124;
const STAGE4_USER_SYSCALL_EXIT_GROUP_NR : usize = 94;
const STAGE4_USER_IMAGE_BASE : usize = 0x1000_0000;
const STAGE4_USER_IMAGE_SIZE : usize = 0x2000;
const STAGE4_USER_STACK_TOP : usize = 0x0000_0000_7FFF_F000;
const STAGE4_USER_STACK_SIZE : usize = 16 * 1024;

/// 仅参数 `a7` 的 `ecall`；返回值在 `a0`，约定与内核对用户态 syscall 返回一致。
#[inline]
unsafe fn user_syscall0(nr : usize) -> isize {
    let mut ret = 0usize;
    unsafe {
        asm!("ecall",
             inlateout("a0") ret,
             in("a7") nr,
             clobber_abi("C"));
    }
    ret as isize
}

/// 走 `exit_group` 号直接 `noreturn` ecall，用于用户态自检片段无返回退出。
#[inline]
unsafe fn user_exit_group(exit_code : isize) -> ! {
    unsafe {
        asm!("ecall",
             in("a0") exit_code as usize,
             in("a7") STAGE4_USER_SYSCALL_EXIT_GROUP_NR,
             options(noreturn));
    }
}

// 纯自旋延迟，避免日志在极短 tick 内刷屏；无硬件假设。
fn busy_delay(rounds : usize) {
    for _ in 0..rounds {
        core::hint::spin_loop();
    }
}

/// 若存在当前任务，打印调度统计；用于 sleep/block 路径的可观测性。
fn log_task_snapshot(label : &str) {
    if let Some(snapshot) = task::current_task_snapshot() {
        info!("[task-stage2] {} id={} state={:?} schedule_count={} tick_count={}",
              label,
              snapshot.id,
              snapshot.state,
              snapshot.stats
                      .schedule_count,
              snapshot.stats
                      .tick_count);
    } else {
        warn!("[task-stage2] {} no current task snapshot",
              label);
    }
}

/// stage2：`sleep_for_ticks` 与快照日志交替。
extern "C" fn stage2_sleep_task(_arg : usize) -> ! {
    info!("[task-stage2] sleep task started");
    for round in 1..=3usize {
        log_task_snapshot("sleep-before");
        info!("[task-stage2] sleep task round {} -> sleep_for_ticks(2)",
              round);
        task::sleep_for_ticks(2);
        log_task_snapshot("sleep-after");
        busy_delay(250_000);
    }
    info!("[task-stage2] sleep task exiting with code 11");
    task::exit_current(11);
}

/// stage2：到达 `block_current` 前释放 `BLOCK_TASK_READY`，供 waker 同步。
extern "C" fn stage2_blocked_task(_arg : usize) -> ! {
    info!("[task-stage2] blocked task started");
    log_task_snapshot("block-before");
    BLOCK_TASK_READY.store(true, Ordering::Release);
    info!("[task-stage2] blocked task entering manual block");
    task::block_current(task::TaskBlockReason::Manual);
    info!("[task-stage2] blocked task resumed after wake");
    log_task_snapshot("block-after");
    busy_delay(250_000);
    info!("[task-stage2] blocked task exiting with code 22");
    task::exit_current(22);
}

/// stage2：等待 ready 标志后延迟 tick 再 `wake_task`，验证手动阻塞/唤醒序。
extern "C" fn stage2_waker_task(blocked_task_id : usize) -> ! {
    info!("[task-stage2] waker task started, target blocked_task_id={}",
          blocked_task_id);
    let mut attempt = 0usize;
    while !BLOCK_TASK_READY.load(Ordering::Acquire) {
        attempt = attempt.wrapping_add(1);
        if attempt % 2 == 0 {
            info!("[task-stage2] waker waiting for blocked task to reach block point, attempt={}",
                  attempt);
        }
        task::yield_now();
    }

    info!("[task-stage2] waker observed blocked task ready, delaying wake by 3 ticks");
    task::sleep_for_ticks(3);
    let woke = task::wake_task(blocked_task_id);
    info!("[task-stage2] waker invoked wake_task({}) -> {}",
          blocked_task_id, woke);
    log_task_snapshot("waker-after-wake");

    for round in 1..=3usize {
        info!("[task-stage2] waker observation round {}",
              round);
        task::yield_now();
    }

    info!("[task-stage2] waker task exiting with code 33");
    task::exit_current(33);
}

/// stage3b：在 wait queue 上应超时返回。
extern "C" fn stage3_wait_timeout_task(wait_queue_id : usize) -> ! {
    info!("[task-stage3b] timeout task started, wait_queue_id={}",
          wait_queue_id);
    let wait_result = task::wait_on_for_ticks(task::TaskWaitHandle::for_wait_queue(wait_queue_id),
                                              2);
    assert_eq!(wait_result,
               task::TaskWaitResult::TimedOut,
               "wait queue timeout task must observe TimedOut");
    info!("[task-stage3b] timeout task observed {:?} as expected",
          wait_result);
    task::exit_current(44);
}

/// stage3b：先睡再退出，供 exit-waiter / reaper 验证。
extern "C" fn stage3_exit_target_task(_arg : usize) -> ! {
    info!("[task-stage3b] exit-target task started");
    task::sleep_for_ticks(2);
    info!("[task-stage3b] exit-target task exiting with code 55");
    task::exit_current(55);
}

/// stage3b：`wait_for_task_exit_for_ticks` 须在时限内被目标退出唤醒。
extern "C" fn stage3_exit_waiter_task(exit_target_task_id : usize) -> ! {
    info!("[task-stage3b] exit-waiter task started, target={}",
          exit_target_task_id);
    let wait_result = task::wait_for_task_exit_for_ticks(exit_target_task_id, 8);
    assert_eq!(wait_result,
               task::TaskWaitResult::Woken,
               "wait-for-exit task must be woken by target exit");
    info!("[task-stage3b] exit-waiter observed target {} exit via {:?}",
          exit_target_task_id, wait_result);
    task::exit_current(66);
}

/// stage3b：稍后 `reap_exited_task`，断言僵尸元数据与退出码。
extern "C" fn stage3_exit_reaper_task(exit_target_task_id : usize) -> ! {
    info!("[task-stage3b] exit-reaper task started, target={}",
          exit_target_task_id);
    task::sleep_for_ticks(5);
    let exited_task = task::reap_exited_task(exit_target_task_id).expect("exit-reaper must \
                                                                          observe zombie task \
                                                                          before reaping");
    assert_eq!(exited_task.id, exit_target_task_id,
               "reaped task id must match exit target");
    assert_eq!(exited_task.exit_code, 55,
               "reaped task exit code must match exit target");
    info!("[task-stage3b] exit-reaper reaped task {} with exit_code={}",
          exited_task.id, exited_task.exit_code);
    task::exit_current(77);
}

/// 映射到用户特权级的最小片段：`sched_yield` 成功则 `exit_group(OK)`，否则错误码路径。
extern "C" fn stage4_user_task_entry() -> ! {
    let yield_ret = unsafe { user_syscall0(STAGE4_USER_SYSCALL_YIELD_NR) };
    let exit_code = if yield_ret == 0 {
        STAGE4_USER_EXIT_OK
    } else {
        STAGE4_USER_EXIT_BAD_RET
    };
    unsafe {
        user_exit_group(exit_code);
    }
}

/// 等待根卷默认 ELF 用户任务退出并 reap，仅日志与资源校验。
extern "C" fn elf_user_observer_task(elf_task_id : usize) -> ! {
    trace!("[elf-selftest] elf observer entered elf_task_id={}",
           elf_task_id);
    info!("[elf-selftest] observer waiting for elf task id={}",
          elf_task_id);
    let _ = task::wait_for_task_exit_for_ticks(elf_task_id, 256);
    if let Some(exited) = task::reap_exited_task(elf_task_id) {
        info!("[elf-selftest] reaped elf task exit_code={}",
              exited.exit_code);
    } else {
        warn!("[elf-selftest] elf task not in zombie state for reaping");
    }
    task::exit_current(100);
}

/// 对 [`stage4_user_task_entry`] 派生任务做 `wait`/`reap` 与 trap/资源快照断言。
extern "C" fn stage4_user_observer_task(user_task_id : usize) -> ! {
    info!("[task-stage4] observer task started, user_task_id={}",
          user_task_id);
    let wait_result = task::wait_for_task_exit_for_ticks(user_task_id, 16);
    assert_eq!(wait_result,
               task::TaskWaitResult::Woken,
               "stage4 observer must observe user task exit");
    let exited_task =
        task::reap_exited_task(user_task_id).expect("stage4 observer must reap exited user task");
    assert_eq!(exited_task.id, user_task_id,
               "reaped user task id must match spawned user task");
    assert_eq!(exited_task.kind,
               task::TaskKind::User,
               "reaped task must be a user task");
    assert_eq!(exited_task.exit_code, STAGE4_USER_EXIT_OK,
               "user task must observe sched_yield return 0 before exit");
    assert!(exited_task.trap_frame
                       .is_some(),
            "user task exit should leave an observable trap frame snapshot");
    let user_resources =
        exited_task.user_resources
                   .expect("user task exit should preserve user resources snapshot");
    assert_eq!(user_resources.entry_pc, stage4_user_task_entry as *const () as usize,
               "user task resource snapshot must preserve original entry pc");
    assert!(user_resources.user_stack_bottom < user_resources.user_stack_top,
            "user task stack range must be ordered");
    assert_eq!(user_resources.user_stack_top - user_resources.user_stack_bottom,
               user_resources.user_stack_size,
               "user task stack range must match reported stack size");
    assert_eq!(user_resources.address_space,
               Some(task::AddressSpaceHandle::from_raw(mm::kernel_mm::kernel_satp())),
               "user task resource snapshot must preserve address-space handle metadata");
    let image =
        user_resources.image
                      .expect("user task resource snapshot must preserve user image metadata");
    assert_eq!(image.image_base(),
               STAGE4_USER_IMAGE_BASE,
               "user task image metadata must preserve image base");
    assert_eq!(image.image_size(),
               STAGE4_USER_IMAGE_SIZE,
               "user task image metadata must preserve image size");
    let trap_frame = exited_task.trap_frame
                                .expect("user task should preserve trap frame");
    let user_sp = trap_frame.user_sp();
    assert!(user_resources.user_stack_bottom <= user_sp,
            "user task trap frame user_sp must stay within the user stack range");
    assert!(user_sp <= user_resources.user_stack_top,
            "user task trap frame user_sp must not grow beyond the initial user stack top");
    assert_eq!(user_sp & 0xF,
               0,
               "user task trap frame user_sp should keep 16-byte stack alignment");
    info!("[task-stage4] observer reaped user task {} with exit_code={}",
          exited_task.id, exited_task.exit_code);
    task::exit_current(99);
}

/// 先启动根卷默认 ELF 用户任务，再启动 stage2/3/4
/// 自检任务，降低自检对真实用户程序的调度干扰。
pub fn spawn_all() {
    #[cfg(feature = "impl-sv39")]
    {
        trace!("[elf-selftest] try load ELF path={} (before scheduler self-tests)",
               mm::kernel_mm::DEFAULT_USER_ELF_PATH);
        match mm::kernel_mm::from_elf_path(mm::kernel_mm::DEFAULT_USER_ELF_PATH) {
            Ok(loaded) => {
                trace!("[elf-selftest] load ok entry={:#x} satp={:#x} image=[{:#x},+{:#x}) \
                        stack=({:#x},{:#x}]",
                       loaded.entry_pc,
                       loaded.satp,
                       loaded.image_base,
                       loaded.image_size,
                       loaded.stack_bottom,
                       loaded.stack_top);
                let elf_task_id = task::spawn_user_task_spec(
                    task::UserTaskSpec::new(loaded.entry_pc)
                        .with_address_space(task::AddressSpaceHandle::from_raw(loaded.satp))
                        .with_image(task::UserImageInfo::new(
                            loaded.image_base,
                            loaded.image_size,
                        ))
                        .with_external_stack(loaded.stack_bottom, loaded.stack_top),
                );
                let _elf_obs = task::spawn_kernel_task(elf_user_observer_task, elf_task_id);
                info!("[elf-selftest] spawned ELF user task id={} path={}",
                      elf_task_id,
                      mm::kernel_mm::DEFAULT_USER_ELF_PATH);
            }
            Err(err) => {
                // 用 info：默认日志级别下须可见；否则易误以为「hello 消失」是 trap 等问题。
                info!(
                    "[elf-selftest] default hello ELF not loaded (no stdout from 000_hello_world). \
                     err={:?} path={} — only kernel-mapped stage4 user self-test runs",
                    err,
                    mm::kernel_mm::DEFAULT_USER_ELF_PATH
                );
            }
        }

        mm::kernel_mm::ensure_user_execute_for_kernel_va(stage4_user_task_entry as *const ()
                                                         as usize);
        let stack_bottom = VirtAddr(STAGE4_USER_STACK_TOP - STAGE4_USER_STACK_SIZE);
        let stack_top = VirtAddr(STAGE4_USER_STACK_TOP);
        mm::kernel_mm::map_anon_range_user(stack_bottom,
                                           stack_top,
                                           PagePerm::R | PagePerm::W | PagePerm::U);
    }

    let wait_timeout_queue = task::WaitQueue::new();
    let blocked_task_id = task::spawn_kernel_task(stage2_blocked_task, 0);
    let sleep_task_id = task::spawn_kernel_task(stage2_sleep_task, 0);
    let waker_task_id = task::spawn_kernel_task(stage2_waker_task, blocked_task_id);
    let wait_timeout_task_id = task::spawn_kernel_task(stage3_wait_timeout_task,
                                                       wait_timeout_queue.id());
    let exit_target_task_id = task::spawn_kernel_task(stage3_exit_target_task, 0);
    let exit_waiter_task_id = task::spawn_kernel_task(stage3_exit_waiter_task,
                                                      exit_target_task_id);
    let exit_reaper_task_id = task::spawn_kernel_task(stage3_exit_reaper_task,
                                                      exit_target_task_id);
    let stage4_entry = stage4_user_task_entry as *const () as usize;
    trace!("[task-stage4] spawn kernel-mapped user task entry={:#x} kernel_satp={:#x} \
            image=[{:#x},+{:#x}) stack_top={:#x}",
           stage4_entry,
           mm::kernel_mm::kernel_satp(),
           STAGE4_USER_IMAGE_BASE,
           STAGE4_USER_IMAGE_SIZE,
           STAGE4_USER_STACK_TOP);
    let user_task_id = task::spawn_user_task_spec(
        task::UserTaskSpec::new(stage4_entry)
            .with_address_space(task::AddressSpaceHandle::from_raw(
                mm::kernel_mm::kernel_satp(),
            ))
            .with_image(task::UserImageInfo::new(
                STAGE4_USER_IMAGE_BASE,
                STAGE4_USER_IMAGE_SIZE,
            ))
            .with_external_stack(
                STAGE4_USER_STACK_TOP - STAGE4_USER_STACK_SIZE,
                STAGE4_USER_STACK_TOP,
            ),
    );
    trace!("[task-stage4] kernel-mapped user_task_id={}",
           user_task_id);
    let user_observer_task_id = task::spawn_kernel_task(stage4_user_observer_task, user_task_id);
    info!("[task-stage2] spawned kernel tasks: blocked={}, sleep={}, waker={}",
          blocked_task_id, sleep_task_id, waker_task_id);
    info!("[task-stage3b] spawned self-test tasks: timeout={}, exit_target={}, exit_waiter={}, \
           exit_reaper={}, wait_queue_id={}",
          wait_timeout_task_id,
          exit_target_task_id,
          exit_waiter_task_id,
          exit_reaper_task_id,
          wait_timeout_queue.id());
    info!("[task-stage4] spawned user self-test tasks: user={}, observer={}",
          user_task_id, user_observer_task_id);
}
