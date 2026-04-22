#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

#[panic_handler]
pub fn panic_handler(_panic_info : &core::panic::PanicInfo) -> ! {
    runtime::panic::panic_handler(_panic_info)
}

#[alloc_error_handler]
pub fn alloc_error_handler(layout : core::alloc::Layout) -> ! {
    runtime::heap_allocator::handle_alloc_error(layout)
}

#[cfg(feature = "qemu-riscv64-opensbi")]
mod qemu_riscv64_opensbi {
    use core::arch::global_asm;
    use core::include_str;
    use core::sync::atomic::{AtomicBool, Ordering};
    use runtime::logging::*;
    global_asm!(include_str!("../components/wateros-platform/platform-impl/\
                              impl-qemu-riscv64-opensbi/src/asm/_start.S"));

    static BLOCK_TASK_READY: AtomicBool = AtomicBool::new(false);

    fn busy_delay(rounds: usize) {
        for _ in 0..rounds {
            core::hint::spin_loop();
        }
    }

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

    extern "C" fn stage2_sleep_task(_arg: usize) -> ! {
        info!("[task-stage2] sleep task started");
        for round in 1..=3usize {
            log_task_snapshot("sleep-before");
            info!("[task-stage2] sleep task round {} -> sleep_for_ticks(2)", round);
            task::sleep_for_ticks(2);
            log_task_snapshot("sleep-after");
            busy_delay(250_000);
        }
        info!("[task-stage2] sleep task exiting with code 11");
        task::exit_current(11);
    }

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
            blocked_task_id,
            woke
        );
        log_task_snapshot("waker-after-wake");

        for round in 1..=3usize {
            info!("[task-stage2] waker observation round {}", round);
            task::yield_now();
        }

        info!("[task-stage2] waker task exiting with code 33");
        task::exit_current(33);
    }

    #[unsafe(no_mangle)]
    pub fn kernel_main(boot_arg0 : usize, boot_arg1 : usize) -> ! {
        use platform::boot::{BootArgs, BootContext};
        let _boot_context = BootContext::from(BootArgs::new(boot_arg0, boot_arg1));
        driver::init_when_boot(boot_arg1);
        runtime::console::show_logo();
        runtime::logging::init();
        info!("log test pass!");
        runtime::heap_allocator::init();
        use alloc::vec;
        let vec_test = vec![0; 10];
        debug!("vec_test = {:?}", vec_test);

        platform::arch::init();

        // ===== 内核态自检：MM / FrameAllocator / Sv39 =====
        unsafe extern "C" {
            fn kernel_end();
        }
        // QEMU virt 默认 RAM：0x8000_0000..0x8800_0000（与 old 代码一致）
        const MEMORY_END: usize = 0x8800_0000;
        const PAGE_SIZE: usize = 4096;
        #[inline]
        const fn align_up(v: usize, align: usize) -> usize {
            (v + align - 1) & !(align - 1)
        }
        let start_ppn = align_up(kernel_end as usize, PAGE_SIZE) / PAGE_SIZE;
        let end_ppn = MEMORY_END / PAGE_SIZE;
        info!(
            "[self-test] frame range ppn=[{:#x},{:#x})",
            start_ppn,
            end_ppn
        );
        mm::test_with_range(
            base::addr::BasePPN { val: start_ppn },
            base::addr::BasePPN { val: end_ppn },
        );
        info!("[self-test] mm self-test done");

        // 设备驱动扫描与根文件系统挂载自检。
        if let Err(err) = driver::active_impl::init_after_boot() {
            warn!("[self-test] driver init failed: {:?}", err);
        } else {
            info!("[self-test] driver init done");
            fs::init();
            fs::test();
        }

        task::init();
        let blocked_task_id = task::spawn_kernel_task(stage2_blocked_task, 0);
        let sleep_task_id = task::spawn_kernel_task(stage2_sleep_task, 0);
        let waker_task_id = task::spawn_kernel_task(stage2_waker_task, blocked_task_id);
        info!(
            "[task-stage2] spawned kernel tasks: blocked={}, sleep={}, waker={}",
            blocked_task_id,
            sleep_task_id,
            waker_task_id
        );

        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        info!("[task-stage2] starting first task");
        task::run_first_task()
    }
}
