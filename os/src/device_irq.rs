//! 设备硬中断与内核工作任务的接线；硬中断不进入协议栈或设备对象锁。

use core::sync::atomic::{AtomicUsize, Ordering};

static IRQ_CPU : AtomicUsize = AtomicUsize::new(usize::MAX);
static NETWORK_QUEUE : AtomicUsize = AtomicUsize::new(usize::MAX);
static NETWORK_GENERATION : AtomicUsize = AtomicUsize::new(0);
static CONSOLE_QUEUE : AtomicUsize = AtomicUsize::new(usize::MAX);
static CONSOLE_GENERATION : AtomicUsize = AtomicUsize::new(0);

pub fn init_console_wait() {
    let queue = task::wait_queue::WaitQueue::new_named("console-irq");
    CONSOLE_QUEUE.store(queue.id(), Ordering::Release);
}

pub fn console_generation() -> usize { CONSOLE_GENERATION.load(Ordering::Acquire) }

pub fn wait_console(observed : usize) {
    let target = task::TaskWaitTarget::WaitQueue(CONSOLE_QUEUE.load(Ordering::Acquire));
    if driver::irq::uart_irq_ready() {
        task::wait_on_while(target, || {
            console_generation() == observed
        });
    } else {
        task::wait_on_while_for_ticks(target, 1, || {
            console_generation() == observed
        });
    }
}

fn notify_console() {
    CONSOLE_GENERATION.fetch_add(1, Ordering::AcqRel);
    let queue = CONSOLE_QUEUE.load(Ordering::Acquire);
    if queue != usize::MAX {
        task::wake_one_in_wait_queue_deferred(queue);
    }
}

/// BSP 在关中断且内核 MMIO 映射就绪后调用，早于设备枚举。
pub fn init() -> Result<(), &'static str> {
    let cpu = platform::arch::cpu::current_cpu_id().raw();
    platform::irq::init(cpu)?;
    IRQ_CPU.store(cpu, Ordering::Release);
    Ok(())
}

pub fn init_network_wait() {
    let queue = task::wait_queue::WaitQueue::new_named("network-irq");
    NETWORK_QUEUE.store(queue.id(), Ordering::Release);
    network::stack::set_activity_notifier(notify_network);
}

pub fn network_generation() -> usize { NETWORK_GENERATION.load(Ordering::Acquire) }

pub fn notify_network() {
    NETWORK_GENERATION.fetch_add(1, Ordering::AcqRel);
    let queue = NETWORK_QUEUE.load(Ordering::Acquire);
    if queue != usize::MAX {
        task::wake_one_in_wait_queue_deferred(queue);
    }
}

/// 条件复查与挂起由调度器在同一个临界区内执行，避免 IRQ 落在检查与睡眠之间。
pub fn wait_network(observed : usize, timeout_ticks : Option<u64>) {
    let queue = NETWORK_QUEUE.load(Ordering::Acquire);
    let target = task::TaskWaitTarget::WaitQueue(queue);
    match timeout_ticks {
        Some(ticks) => {
            task::wait_on_while_for_ticks(target, ticks.max(1), || {
                network_generation() == observed
            });
        }
        None => {
            task::wait_on_while(target, || {
                network_generation() == observed
            });
        }
    }
}

/// 有界 drain，防止持续到达的设备事件阻止 timer/IPI 处理。
pub fn dispatch() {
    for _ in 0..64 {
        let Some(irq) = platform::irq::claim() else {
            break;
        };
        let events = driver::irq::handle_irq(irq);
        if !driver::irq::is_registered(irq) {
            platform::irq::disable(irq);
        }
        platform::irq::complete(irq);
        if events & driver::irq::NETWORK != 0 {
            notify_network();
        }
        if events & driver::irq::UART != 0 {
            notify_console();
        }
    }
}

/// 同步 I/O 可能持有 FS/设备锁，等待期间不得调度。只在 IRQ 路由 CPU 使用 WFI；
/// 其它 CPU 检查 DMA used ring，避免依赖正在等待其锁的 BSP 来处理 IRQ。
pub fn wait_for_device(ready : &mut dyn FnMut() -> bool) {
    let state = platform::arch::interrupt::read_global_interrupt_state().expect("read IRQ state \
                                                                                 for device wait");
    platform::arch::interrupt::disable_global_interrupt().expect("mask device wait IRQs");
    let receives_irq =
        platform::arch::cpu::current_cpu_id().raw() == IRQ_CPU.load(Ordering::Acquire);
    while !ready() {
        // WFI/idle 可由已使能的 pending 源唤醒，无需开启全局中断；这样也关闭了
        // completion 检查与睡眠之间被 handler 提前 ACK 的窗口。
        if receives_irq {
            platform::arch::interrupt::wait_for_interrupt();
        } else {
            core::hint::spin_loop();
        }
    }
    platform::arch::interrupt::restore_global_interrupt_state(state).expect("restore IRQ state \
                                                                             after device wait");
}
