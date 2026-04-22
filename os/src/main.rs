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
    use runtime::logging::*;
    global_asm!(include_str!("../components/wateros-platform/platform-impl/\
                              impl-qemu-riscv64-opensbi/src/asm/_start.S"));
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

        platform::interrupt::enable_global_interrupt().unwrap();
        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        loop {}


        unreachable!("unreachable code in platform_main")
    }
}
