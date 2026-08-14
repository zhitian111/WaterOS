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

extern "C" fn bottom_half_task(_arg : usize) -> ! {
    let queue = BH_QUEUE.call_once(|| WaitQueue::new_named("irq-bottom-half"));
    loop {
        platform::irq::bottom_half::run_pending();
        // 条件为「无待处理工作」时睡眠；调度器在临界区内复查条件，避免丢失唤醒。
        queue.wait_current_while(|| !platform::irq::bottom_half::has_pending());
    }
}

/// 注册唤醒回调并启动 bottom-half 内核任务；须在 `task::init()` 之后调用。
pub fn init() {
    platform::irq::bottom_half::set_wake_callback(wake_bottom_half);
    task::spawn_kernel_task(bottom_half_task, 0);
    runtime::logging::info!("[irq] bottom-half task started");
}
