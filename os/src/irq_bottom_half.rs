//! IRQ bottom-half 内核任务：在可调度上下文运行 pending 的 bottom-half 工作。
//!
//! ISR（top-half）只做 ack/标记；真正回收 virtqueue 与唤醒等待任务的工作由本
//! 任务在 waitqueue 上等待、被唤醒后执行，避免在中断上下文 sleep 或拿阻塞锁。

use spin::Once;
use task::WaitQueue;

static BH_QUEUE : Once<WaitQueue> = Once::new();

fn wake_bottom_half() {
    if let Some(queue) = BH_QUEUE.get() {
        let _ = queue.wake_one();
    }
}

/// 保存当前全局中断状态并打开「仅外部中断可投递」窗口（屏蔽 timer/soft）。
///
/// 等待窗口内屏蔽定时器/软中断，避免持 FS 自旋锁的等待任务被抢占导致其它核
/// 永久自旋；恢复时按调用前状态还原。
fn irq_save_and_enable_external() -> usize {
    let state = platform::interrupt::read_global_interrupt_state()
        .map(|state| state.0)
        .unwrap_or(0);
    let _ = platform::arch::interrupt::disable_timer_interrupt();
    let _ = platform::arch::interrupt::disable_soft_interrupt();
    let _ = platform::interrupt::enable_global_interrupt();
    state
}

fn irq_restore(state : usize) {
    let _ = platform::arch::interrupt::enable_timer_interrupt();
    let _ = platform::arch::interrupt::enable_soft_interrupt();
    let _ = platform::interrupt::restore_global_interrupt_state(
        platform::arch::interrupt::ArchInterruptState(state));
}

extern "C" fn bottom_half_task(_arg : usize) -> ! {
    let queue = BH_QUEUE.call_once(|| WaitQueue::new_named("irq-bottom-half"));
    loop {
        platform::irq::bottom_half::run_pending();
        // 条件为「无待处理工作」时睡眠；调度器在临界区内复查条件避免丢失唤醒。
        // 高频率 IRQ 下偶发唤醒丢失时，1 tick 超时兜底重查，杜绝永久卡死。
        queue.wait_current_while_for_ticks(1,
                                           || !platform::irq::bottom_half::has_pending());
    }
}

/// 注册唤醒回调并启动 bottom-half 内核任务；须在 `task::init()` 之后调用。
pub fn init() {
    platform::irq::bottom_half::set_wake_callback(wake_bottom_half);
    platform::irq::wait::set_local_irq_ops(irq_save_and_enable_external, irq_restore);
    task::spawn_kernel_task(bottom_half_task, 0);
    runtime::logging::info!("[irq] bottom-half task started");
}
