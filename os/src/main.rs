#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;
use syscall as _;

#[cfg(feature = "qemu-riscv64-opensbi")]
mod self_tests;

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
    use mm::api::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
    use mm::api::address_space::AddressSpaceOps;
    use mm::api::perm::PagePerm;
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
        const MEMORY_END : usize = 0x8800_0000;
        const PAGE_SIZE : usize = 4096;
        #[inline]
        const fn align_up(v : usize, align : usize) -> usize { (v + align - 1) & !(align - 1) }
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
        paging_effective_smoke_test(start_ppn, end_ppn);
        info!("[self-test] mm self-test done");

        // 设备驱动扫描与根文件系统挂载自检。
        if let Err(err) = driver::active_impl::init_after_boot() {
            warn!("[self-test] driver init failed: {:?}",
                  err);
        } else {
            info!("[self-test] driver init done");
            fs::init();
            fs::test();
        }

        task::init();
        crate::self_tests::task::spawn_all();

        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        info!("[task-selftest] starting first task");
        task::run_first_task()
    }

    fn paging_effective_smoke_test(start_ppn: usize, end_ppn: usize) {
        let _ = (start_ppn, end_ppn);
        #[cfg(feature = "impl-sv39")]
        {
            let mut aspace = mm::mm_impl::Sv39AddressSpace::new()
                .expect("paging smoke: create address space");

            // 先建立内核执行区间的恒等映射，保证切换 satp 后指令流不中断。
            let kernel_start_vpn = VirtAddr(0x8000_0000).floor_page();
            let kernel_end_vpn = VirtAddr(0x8800_0000).ceil_page();
            for vpn_raw in kernel_start_vpn.0..kernel_end_vpn.0 {
                let vpn = VirtPageNum(vpn_raw);
                let ppn = vpn.to_phys_page();
                aspace
                    .map_page_to_ppn(vpn, ppn, PagePerm::R | PagePerm::W | PagePerm::X)
                    .expect("paging smoke: identity map ram");
            }

            // 构造一个仅由页表提供的高地址映射，作为“分页生效”探针。
            assert!(start_ppn + 16 < end_ppn, "paging smoke: probe ppn out of range");
            let probe_ppn = PhysPageNum(start_ppn + 16);
            let probe_va = VirtAddr(0x4000_0000usize + 0x2A0);
            let probe_vpn = probe_va.floor_page();
            aspace
                .map_page_to_ppn(probe_vpn, probe_ppn, PagePerm::R | PagePerm::W)
                .expect("paging smoke: map probe page");

            let satp_before = platform::arch::paging::read_satp();
            let satp_target = aspace.satp_value();
            info!(
                "[self-test][paging] satp before={:#x}, target={:#x}",
                satp_before,
                satp_target
            );
            platform::arch::paging::write_satp_and_flush(satp_target);
            let satp_after = platform::arch::paging::read_satp();
            info!("[self-test][paging] satp after={:#x}", satp_after);
            assert_eq!(satp_after, satp_target);

            let probe_ptr = probe_va.0 as *mut u64;
            unsafe { probe_ptr.write_volatile(0x1122_3344_5566_7788); }
            let probe_pa = PhysAddr(probe_ppn.0 * PAGE_SIZE + probe_va.page_offset());
            let phys_ptr = probe_pa.0 as *const u64;
            let observed = unsafe { phys_ptr.read_volatile() };
            assert_eq!(observed, 0x1122_3344_5566_7788);
            info!(
                "[self-test][paging] mapped probe write ok: va={:#x} -> pa={:#x}",
                probe_va.0,
                probe_pa.0
            );

            // 默认不触发 fault，避免打断启动流程；调试时可改为 true 观察 trap 行为。
            const ENABLE_FAULT_PROBE: bool = false;
            if ENABLE_FAULT_PROBE {
                let fault_va = VirtAddr(0x5000_0000usize);
                info!(
                    "[self-test][paging] trigger load page fault: va={:#x}",
                    fault_va.0
                );
                let fault_ptr = fault_va.0 as *const u64;
                let _ = unsafe { fault_ptr.read_volatile() };
            } else {
                info!("[self-test][paging] fault probe skipped (ENABLE_FAULT_PROBE=false)");
            }
        }
    }
}
