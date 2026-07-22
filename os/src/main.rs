#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use klog as _;
use runtime::logging::warn;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
use syscall as _;

mod boot_timebase;
mod trap_handler;
mod user_bringup_bus;
mod user_bringup_busybox;
mod user_bringup_common;
mod user_bringup_mm;
mod user_bringup_posix_fs;
mod user_bringup_root_layout;

// ── Panic / Alloc ───────────────────────────────────────────────

#[panic_handler]
pub fn panic_handler(_panic_info : &core::panic::PanicInfo) -> ! {
    runtime::panic::panic_handler(_panic_info)
}

#[alloc_error_handler]
pub fn alloc_error_handler(layout : core::alloc::Layout) -> ! {
    runtime::heap_allocator::handle_alloc_error(layout)
}

// ── 共享 bring-up ──────────────────────────────────────────────

/// 网络协议栈轮询任务：周期性驱动 smoltcp 收发包。
extern "C" fn network_poller_task(_arg : usize) -> ! {
    loop {
        match platform::timer::now_duration() {
            Ok(now) => {
                let millis = now.as_millis()
                                .min(i64::MAX as u128) as i64;
                driver::network::stack::poll_at_millis(millis);
            }
            Err(_) => driver::network::stack::poll(),
        }
        driver::network::stack::poll_socket_events();
        task::sleep_for_ticks(1);
    }
}

/// 驱动 → 网络 → FS → 用户态 bring-up。两 board 模块共用。
fn bringup_driver_and_user() {
    match driver::active_impl::init_after_boot() {
        Err(ref err) => warn!("driver init failed: {:?}", err),
        Ok(()) => {
            let _ = driver::network::stack::init([10, 0, 2, 15], [10, 0, 2, 2]).inspect(|_| {
                        task::spawn_kernel_task(network_poller_task, 0);
                    })
                    .inspect_err(|e| warn!("network stack init skipped: {}", e));
            fs::init();
            crate::user_bringup_bus::run();
            fs::test();
            #[cfg(feature = "vfs-bridge")]
            vfs::test();
        }
    }
}

#[cfg(feature = "qemu-riscv64-opensbi")]
mod qemu_riscv64_opensbi {
    use crate::bringup_driver_and_user;
    use core::arch::global_asm;
    use core::include_str;
    use core::sync::atomic::{AtomicBool, Ordering};
    use runtime::logging::*;
    global_asm!(include_str!("../components/wateros-platform/platform-impl/\
                              impl-qemu-riscv64-opensbi/src/asm/_start.S"));
    /// 首个到达 `kernel_main` 的 hart 用 swap(true) 抢占 BSP 身份。
    static BSP_CLAIMED : AtomicBool = AtomicBool::new(false);
    /// BSP 完成初始化后置 true，AP 自旋等待此标志。
    static AP_BOOT_READY : AtomicBool = AtomicBool::new(false);
    /// AP 入口：BSP 初始化完成后被调用；局部初始化后加入调度。
    fn ap_main(cpu_id : task::CpuId) -> ! {
        warn!("[boot-init] AP cpu={} entering scheduler",
              cpu_id.raw());
        platform::arch::cpu::init_current_cpu(cpu_id).expect("AP init current CPU");
        platform::arch::init();
        // 开 AP 定时器中断，使 idle 能被 tick 唤醒从而从全局就绪队列取任务
        platform::interrupt::enable_timer_interrupt().expect("AP enable timer interrupt");
        platform::timer::set_timer_after_ms(100).expect("AP set initial timer");
        task::set_cpu_online(cpu_id);
        task::run_first_task_on_current_cpu(cpu_id)
    }

    /// AP 在 BSP 完成初始化前自旋等待 `AP_BOOT_READY` 标志。
    fn wait_ap_boot_ready(cpu_id : task::CpuId) -> ! {
        while !AP_BOOT_READY.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        ap_main(cpu_id)
    }

    /// 屏蔽固件遗留的中断。
    fn mask_boot_interrupts() {
        platform::interrupt::disable_global_interrupt().expect("disable global interrupt");
        platform::interrupt::disable_timer_interrupt().expect("disable timer interrupt");
    }

    #[unsafe(no_mangle)]
    pub fn kernel_main(boot_arg0 : usize, boot_arg1 : usize) -> ! {
        mask_boot_interrupts();
        // 原子 swap 决定 BSP：首个到达的 hart 成为 BSP，其余为 AP
        if BSP_CLAIMED.swap(true, Ordering::AcqRel) {
            let cpu_id = task::CpuId::from_raw(boot_arg0);
            wait_ap_boot_ready(cpu_id);
        }
        // BSP 初始化：驱动 → 日志 → timebase → 堆 → arch → 任务 → trap
        driver::init_when_boot(boot_arg1);
        runtime::console::show_logo();
        klog::init();
        runtime::logging::init();
        crate::boot_timebase::probe_and_init_timebase(boot_arg1);
        runtime::heap_allocator::init();
        platform::arch::cpu::init_current_cpu(task::CpuId::from_raw(boot_arg0))
            .expect("BSP init current CPU");
        platform::arch::init();
        task::init();
        crate::trap_handler::init();
        // MM 初始化
        let memory_end = driver::physical_ram_end_exclusive();
        mm::kernel_mm::init(boot_arg1, memory_end);
        AP_BOOT_READY.store(true, Ordering::Release);

        bringup_driver_and_user();
        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        task::run_first_task()
    }
}

#[cfg(feature = "qemu-loongarch64-virt")]
mod qemu_loongarch64_virt {
    use crate::bringup_driver_and_user;
    use core::arch::global_asm;
    use core::include_str;
    use core::sync::atomic::{AtomicBool, Ordering};
    use runtime::logging::*;

    global_asm!(include_str!("../components/wateros-platform/platform-impl/\
                              impl-qemu-loongarch64-virt/src/asm/_start.S"));

    static BSP_CLAIMED : AtomicBool = AtomicBool::new(false);
    static AP_BOOT_READY : AtomicBool = AtomicBool::new(false);

    fn ap_main(cpu_id : task::CpuId) -> ! {
        warn!("[boot-init] AP cpu={} entering scheduler",
              cpu_id.raw());
        platform::arch::cpu::init_current_cpu(cpu_id).expect("AP init current CPU");
        platform::arch::init();
        platform::interrupt::enable_timer_interrupt().expect("AP enable timer interrupt");
        platform::timer::set_timer_after_ms(100).expect("AP set initial timer");
        task::set_cpu_online(cpu_id);
        task::run_first_task_on_current_cpu(cpu_id)
    }

    fn wait_ap_boot_ready(cpu_id : task::CpuId) -> ! {
        while !AP_BOOT_READY.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        ap_main(cpu_id)
    }

    fn mask_boot_interrupts() {
        platform::interrupt::disable_global_interrupt().expect("disable global interrupt");
        platform::interrupt::disable_timer_interrupt().expect("disable timer interrupt");
    }

    #[unsafe(no_mangle)]
    pub fn kernel_main(_argc : usize, _argv : usize, envp : usize) -> ! {
        mask_boot_interrupts();
        if BSP_CLAIMED.swap(true, Ordering::AcqRel) {
            let cpu_id = platform::arch::cpu::current_cpu_id();
            wait_ap_boot_ready(cpu_id);
        }

        runtime::console::show_logo();
        klog::init();
        runtime::logging::init();
        runtime::heap_allocator::init();
        platform::arch::cpu::init_current_cpu(platform::arch::cpu::current_cpu_id())
            .expect("BSP init current CPU");
        platform::arch::init();
        driver::init_when_boot(envp);
        crate::boot_timebase::probe_and_init_timebase(envp);
        task::init();
        crate::trap_handler::init();
        platform::arch::paging::init_paging_disable_mmu();

        let memory_end = driver::physical_ram_end_exclusive();
        mm::kernel_mm::init(envp, memory_end);

        AP_BOOT_READY.store(true, Ordering::Release);

        bringup_driver_and_user();
        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        task::run_first_task()
    }
}
