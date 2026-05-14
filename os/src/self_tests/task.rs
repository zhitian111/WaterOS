//! 任务调度、等待/超时、退出与回收相关的内核自检（内核任务，无固定根卷 ELF / 伪用户进程）。
//!
//! **入口**：[`spawn_all`] 由 `kernel_main` 在驱动与 `fs` 初始化后调用；覆盖 `sleep`/`yield`、
//! `block`/`wake`、等待队列超时、`wait_for_exit`/`reap` 等契约。
//!
//! 根卷上脚本与可执行 ELF 的启动期日志由 `wateros-fs` 在挂载后 `boot_dump_all_paths` 遍历打印，
//! 不在此模块加载或运行用户 ELF。

use core::sync::atomic::{AtomicBool, Ordering};
use runtime::logging::*;

static BLOCK_TASK_READY: AtomicBool = AtomicBool::new(false);

// 纯自旋延迟，避免日志在极短 tick 内刷屏；无硬件假设。
fn busy_delay(rounds: usize) {
    for _ in 0..rounds {
        core::hint::spin_loop();
    }
}

/// 若存在当前任务，打印调度统计；用于 sleep/block 路径的可观测性。
fn log_task_snapshot(label: &str) {
    if let Some(snapshot) = task::current_task_snapshot() {
        info!(
            "[task-stage2] {} id={} state={:?} schedule_count={} tick_count={}",
            label,
            snapshot.id,
            snapshot.state,
            snapshot.stats.schedule_count,
            snapshot.stats.tick_count
        );
    } else {
        warn!("[task-stage2] {} no current task snapshot", label);
    }
}

/// stage2：`sleep_for_ticks` 与快照日志交替。
extern "C" fn stage2_sleep_task(_arg: usize) -> ! {
    info!("[task-stage2] sleep task started");
    for round in 1..=3usize {
        log_task_snapshot("sleep-before");
        info!(
            "[task-stage2] sleep task round {} -> sleep_for_ticks(2)",
            round
        );
        task::sleep_for_ticks(2);
        log_task_snapshot("sleep-after");
        busy_delay(250_000);
    }
    info!("[task-stage2] sleep task exiting with code 11");
    task::exit_current(11);
}

/// stage2：到达 `block_current` 前释放 `BLOCK_TASK_READY`，供 waker 同步。
extern "C" fn stage2_blocked_task(_arg: usize) -> ! {
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
extern "C" fn stage2_waker_task(blocked_task_id: usize) -> ! {
    info!(
        "[task-stage2] waker task started, target blocked_task_id={}",
        blocked_task_id
    );
    let mut attempt = 0usize;
    while !BLOCK_TASK_READY.load(Ordering::Acquire) {
        attempt = attempt.wrapping_add(1);
        if attempt % 2 == 0 {
            info!(
                "[task-stage2] waker waiting for blocked task to reach block point, attempt={}",
                attempt
            );
        }
        task::yield_now();
    }

    info!("[task-stage2] waker observed blocked task ready, delaying wake by 3 ticks");
    task::sleep_for_ticks(3);
    let woke = task::wake_task(blocked_task_id);
    info!(
        "[task-stage2] waker invoked wake_task({}) -> {}",
        blocked_task_id, woke
    );
    log_task_snapshot("waker-after-wake");

    for round in 1..=3usize {
        info!("[task-stage2] waker observation round {}", round);
        task::yield_now();
    }

    info!("[task-stage2] waker task exiting with code 33");
    task::exit_current(33);
}

/// stage3b：在 wait queue 上应超时返回。
extern "C" fn stage3_wait_timeout_task(wait_queue_id: usize) -> ! {
    info!(
        "[task-stage3b] timeout task started, wait_queue_id={}",
        wait_queue_id
    );
    let wait_result = task::wait_on_for_ticks(
        task::TaskWaitHandle::for_wait_queue(wait_queue_id),
        2,
    );
    assert_eq!(
        wait_result,
        task::TaskWaitResult::TimedOut,
        "wait queue timeout task must observe TimedOut"
    );
    info!(
        "[task-stage3b] timeout task observed {:?} as expected",
        wait_result
    );
    task::exit_current(44);
}

/// stage3b：先睡再退出，供 exit-waiter / reaper 验证。
extern "C" fn stage3_exit_target_task(_arg: usize) -> ! {
    info!("[task-stage3b] exit-target task started");
    task::sleep_for_ticks(2);
    info!("[task-stage3b] exit-target task exiting with code 55");
    task::exit_current(55);
}

/// stage3b：`wait_for_task_exit_for_ticks` 须在时限内被目标退出唤醒。
extern "C" fn stage3_exit_waiter_task(exit_target_task_id: usize) -> ! {
    info!(
        "[task-stage3b] exit-waiter task started, target={}",
        exit_target_task_id
    );
    let wait_result = task::wait_for_task_exit_for_ticks(exit_target_task_id, 8);
    assert_eq!(
        wait_result,
        task::TaskWaitResult::Woken,
        "wait-for-exit task must be woken by target exit"
    );
    info!(
        "[task-stage3b] exit-waiter observed target {} exit via {:?}",
        exit_target_task_id, wait_result
    );
    task::exit_current(66);
}

/// stage3b：稍后 `reap_exited_task`，断言僵尸元数据与退出码。
extern "C" fn stage3_exit_reaper_task(exit_target_task_id: usize) -> ! {
    info!(
        "[task-stage3b] exit-reaper task started, target={}",
        exit_target_task_id
    );
    task::sleep_for_ticks(5);
    let exited_task = task::reap_exited_task(exit_target_task_id)
        .expect("exit-reaper must observe zombie task before reaping");
    assert_eq!(
        exited_task.id, exit_target_task_id,
        "reaped task id must match exit target"
    );
    assert_eq!(
        exited_task.exit_code, 55,
        "reaped task exit code must match exit target"
    );
    info!(
        "[task-stage3b] exit-reaper reaped task {} with exit_code={}",
        exited_task.id, exited_task.exit_code
    );
    task::exit_current(77);
}

/// 启动 stage2 / stage3b 内核自检任务。
pub fn spawn_all() {
    let wait_timeout_queue = task::WaitQueue::new();
    let blocked_task_id = task::spawn_kernel_task(stage2_blocked_task, 0);
    let sleep_task_id = task::spawn_kernel_task(stage2_sleep_task, 0);
    let waker_task_id = task::spawn_kernel_task(stage2_waker_task, blocked_task_id);
    let wait_timeout_task_id =
        task::spawn_kernel_task(stage3_wait_timeout_task, wait_timeout_queue.id());
    let exit_target_task_id = task::spawn_kernel_task(stage3_exit_target_task, 0);
    let exit_waiter_task_id =
        task::spawn_kernel_task(stage3_exit_waiter_task, exit_target_task_id);
    let exit_reaper_task_id =
        task::spawn_kernel_task(stage3_exit_reaper_task, exit_target_task_id);
    info!(
        "[task-stage2] spawned kernel tasks: blocked={}, sleep={}, waker={}",
        blocked_task_id, sleep_task_id, waker_task_id
    );
    info!(
        "[task-stage3b] spawned self-test tasks: timeout={}, exit_target={}, exit_waiter={}, \
         exit_reaper={}, wait_queue_id={}",
        wait_timeout_task_id,
        exit_target_task_id,
        exit_waiter_task_id,
        exit_reaper_task_id,
        wait_timeout_queue.id()
    );
}
