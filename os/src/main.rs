//! WaterOS 内核二进制 crate：在 `no_std` / `no_main`
//! 下提供全局错误处理，并在启用 `qemu-riscv64-opensbi` 时挂载 QEMU RISC-V +
//! OpenSBI 的汇编入口与 [`qemu_riscv64_opensbi::kernel_main`] 启动路径。
//!
//! # 启动（bring-up）概要
//!
//! 1. 引导汇编（`wateros-platform` 对应 `_start.S`）将控制权交给
//!    `kernel_main`。
//! 2. 解析引导参数、初始化驱动桩、控制台与日志、堆分配器，再做 `platform::arch`
//!    与 MM
//!   （frame 范围、Sv39、内核页表等；含 `mm` 自检日志）。
//! 3. 初始化任务、注册组合层 trap 路由（`trap_handler::init`）与内核 trap 的
//!    `satp`，随后 `driver::active_impl::init_after_boot`；成功则挂载 `fs`。
//! 4. 在 **`fs::test()` 的 RW 写盘之前** 调用 [`self_tests::task::spawn_all`]，以便
//!    `from_elf_path` 使用的全局 RO ext4 视图与磁盘一致（避免 `BadClass` 等伪解析错误）。
//! 5. 再跑 `fs` /（可选）`vfs` 的 RW 烟测，开启定时器中断后通过 [`task::run_first_task`]
//!    进入多任务调度。
//!
//! **编译范围**：[`self_tests`] 与 [`qemu_riscv64_opensbi`] 仅在 `feature =
//! "qemu-riscv64-opensbi"` 下存在；其他 board 需另行提供入口与链接脚本。
//!
//! # 自检入口
//!
//! 任务与用户态相关自检的统一入口为 [`self_tests::task::spawn_all`]，由
//! `kernel_main` 在驱动与 `fs::init` 成功后、**`fs::test` 写盘之前** 调用；各 stage
//! 的语义与断言见该模块文档。

#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;
#[cfg(feature = "qemu-riscv64-opensbi")]
use syscall as _;
#[cfg(feature = "qemu-loongarch64-virt")]
use syscall as _;

#[cfg(feature = "qemu-riscv64-opensbi")]
mod self_tests;
#[cfg(feature = "qemu-riscv64-opensbi")]
mod trap_handler;

/// 将内核 panic 委托给 `wateros-runtime` 的统一 panic 处理（日志/停机策略由
/// runtime 决定）。
#[panic_handler]
pub fn panic_handler(_panic_info : &core::panic::PanicInfo) -> ! {
    runtime::panic::panic_handler(_panic_info)
}

/// 堆分配失败时委托给 runtime 的全局分配错误处理；语义为不可恢复错误路径。
#[alloc_error_handler]
pub fn alloc_error_handler(layout : core::alloc::Layout) -> ! {
    runtime::heap_allocator::handle_alloc_error(layout)
}

#[cfg(feature = "qemu-riscv64-opensbi")]
mod qemu_riscv64_opensbi {
    //! QEMU `virt` 机器、RISC-V、OpenSBI 下的内核主入口：与平台 asm
    //! 链接，按固定顺序 完成 bring-up 与（可选）驱动/FS/VFS
    //! 自检，最后进入任务自检与调度器。
    use core::arch::global_asm;
    use core::include_str;
    use runtime::logging::*;
    global_asm!(include_str!("../components/wateros-platform/platform-impl/\
                              impl-qemu-riscv64-opensbi/src/asm/_start.S"));

    /// 引导加载器 / OpenSBI 传入的引导参数；与 [`crate`] 顶层文档中的 bring-up
    /// 步骤一致。
    ///
    /// **契约**：在此返回前完成本路径上的初始化与自检日志；正常路径以
    /// [`task::run_first_task`] 转入调度且不返回。
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
        // 与 DTB `/memory` 或 `wateros-base-config::QEMU_VIRT_PHYS_RAM_END` 对齐（如
        // QEMU `-m 256M` → 0x9000_0000）
        let memory_end = driver::physical_ram_end_exclusive();
        const PAGE_SIZE : usize = 4096;
        #[inline]
        const fn align_up(v : usize, align : usize) -> usize { (v + align - 1) & !(align - 1) }
        let start_ppn = align_up(kernel_end as *const () as usize, PAGE_SIZE) / PAGE_SIZE;
        let end_ppn = memory_end / PAGE_SIZE;
        info!("[self-test] frame range ppn=[{:#x},{:#x})",
              start_ppn, end_ppn);
        mm::test_with_range(base::addr::BasePPN { val : start_ppn },
                            base::addr::BasePPN { val : end_ppn });
        mm::kernel_mm::init(start_ppn, end_ppn, memory_end);
        info!("[self-test] mm self-test done");

        task::init();
        crate::trap_handler::init();
        task::init_kernel_trap_satp(mm::kernel_mm::kernel_satp());

        // 设备驱动扫描与根文件系统挂载自检。
        let driver_boot = driver::active_impl::init_after_boot();
        if let Err(ref err) = driver_boot {
            warn!("[self-test] driver init failed: {:?}",
                  err);
        } else {
            info!("[self-test] driver init done");
            fs::init();
            // 须在 `fs::test()` 的 ext4 RW / vfs RW 写盘 **之前** 完成：否则全局 RO
            // ext4-view 与块设备内容不一致，`from_elf_path` 全文件读可能得到错误字节
            //（例如 `LoadElfError::BadClass`）。
            crate::self_tests::task::spawn_all();
            fs::test();
            #[cfg(feature = "vfs-bridge")]
            {
                vfs::test();
                if let Err(err) =
                    vfs::bridge::rw_write_root_verify_via_ro(vfs::bridge::FsKind::Ext4,
                                                             "hello",
                                                             b"hello")
                {
                    warn!("[self-test] vfs rw bridge verify: {:?}",
                          err);
                } else {
                    info!("[self-test] vfs rw bridge verify OK");
                }
            }
        }
        if driver_boot.is_err() {
            crate::self_tests::task::spawn_all();
        }

        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        info!("[task-selftest] starting first task");
        task::run_first_task()
    }
}

#[cfg(feature = "qemu-loongarch64-virt")]
mod qemu_loongarch64_virt {
    //! QEMU LoongArch `virt` 板级的最小 bring-up：与 `impl-qemu-loongarch64-virt` 的 `_start.S`
    //! 链接后进入 [`kernel_main`]，初始化 runtime/任务与两个内核忙等任务，再开定时器中断
    //! 并进入调度。与 RISC-V OpenSBI 路径相比暂无用户态/FS 自检。
    use core::arch::global_asm;
    use core::include_str;
    use runtime::logging::*;

    global_asm!(include_str!("../components/wateros-platform/platform-impl/\
                              impl-qemu-loongarch64-virt/src/asm/_start.S"));

    /// 固件/引导移交后的内核 C 入口；无 boot 参数版本，完成基础初始化后仅跑内核态烟测任务。
    #[unsafe(no_mangle)]
    pub fn kernel_main() -> ! {
        runtime::console::show_logo();
        runtime::logging::init();
        runtime::heap_allocator::init();
        platform::arch::init();
        info!("[loongarch64] boot smoke ok");

        task::init();
        task::spawn_kernel_task(loongarch64_kernel_task_a, 0);
        task::spawn_kernel_task(loongarch64_kernel_task_b, 0);

        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        info!("[loongarch64][task] starting first task");
        task::run_first_task()
    }

    /// 内核自检任务 A：忙循环 + 周期性日志 + `yield_now`，用于验证多任务与时间片。
    extern "C" fn loongarch64_kernel_task_a(_arg : usize) -> ! {
        let mut round = 0usize;
        loop {
            if round % 1_000_000 == 0 {
                info!("[loongarch64][task-a] round={}", round);
            }
            round = round.wrapping_add(1);
            task::yield_now();
        }
    }

    /// 内核自检任务 B：与 [`loongarch64_kernel_task_a`] 对称，增加调度交错覆盖。
    extern "C" fn loongarch64_kernel_task_b(_arg : usize) -> ! {
        let mut round = 0usize;
        loop {
            if round % 1_000_000 == 0 {
                info!("[loongarch64][task-b] round={}", round);
            }
            round = round.wrapping_add(1);
            task::yield_now();
        }
    }
}
