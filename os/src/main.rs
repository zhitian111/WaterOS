//! WaterOS 内核二进制 crate：在 `no_std` / `no_main`
//! 下提供全局错误处理，并按 board feature 挂载对应 QEMU 平台的汇编入口与
//! `kernel_main` 启动路径。
//!
//! # 启动（bring-up）概要
//!
//! 1. 引导汇编（`wateros-platform` 对应 `_start.S`）将控制权交给
//!    `kernel_main`。
//! 2. 解析引导参数、初始化驱动桩；探测 DTB `timebase-frequency` 并
//!    `platform::time::set_frequency_hz`；再初始化控制台与日志、堆分配器，再做
//!    `platform::arch` 与 MM
//!   （frame 范围、Sv39、内核页表等；含 `mm` 自检日志）。
//! 3. 初始化任务、注册组合层 trap 路由（`trap_handler::init`），再做 MM
//!    （frame 范围、Sv39、内核页表等；含 `mm` 自检日志）；随后 `driver::active_impl::init_after_boot`；成功则挂载 `fs`。
//! 4. 在驱动与 `fs::init`（探测 + 注入 impl，不挂载）成功后，先跑
//!    [`user_bringup_bus::run`]：在总线内 **RW 挂载 ext4 根卷**，再按总线内
//!    登记顺序执行 **`stage-basic`**（直接装载 **`/{glibc,musl}/basic/*` ELF**）、
//!    **`stage-busybox`**（busybox + `*_testcode.sh`）等阶段（不需要的注释掉）；
//!    随后 `fs::test()` 等烟测。
//! 5. 开启定时器中断后通过 [`task::run_first_task`] **首次**从引导上下文
//!    `__switch` 到就绪任务；此前 步骤 3 的 `task::init()`
//!    已初始化调度器数据结构，但 **CPU 尚未执行** 任何 spawn
//!    出的任务体（含用户态测程）。
//!
//! **编译范围**：[`self_tests`] 仅在 `feature = "qemu-riscv64-opensbi"`
//! 下存在； board 入口按 `qemu-riscv64-opensbi` / `qemu-loongarch64-virt`
//! 分别编译。
//!
//! # 自检入口
//!
//! 任务相关内核自检的统一入口为 [`self_tests::task::spawn_all`]；用户态
//! bring-up 里程碑 总线为 [`crate::user_bringup_bus::run`]（内含 **basic ELF** /
//! **busybox** 等阶段，注释行调控）；二者在 `kernel_main`
//! 中的先后与语义见模块文档与 `docs/roadmap/riscv64-busybox/wp-init-test-bus.
//! md`。

#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
use klog as _;
#[cfg(feature = "qemu-riscv64-opensbi")]
use syscall as _;
#[cfg(feature = "qemu-loongarch64-virt")]
use syscall as _;

#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod boot_timebase;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod self_tests;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod trap_handler;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod user_bringup_basic;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod user_bringup_bus;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod user_bringup_busybox;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod user_bringup_common;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod user_bringup_mm;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod user_bringup_posix_fs;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
mod user_bringup_root_layout;

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

    /// 网络协议栈轮询任务：周期性驱动 smoltcp 收发包，永久运行。
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
        klog::init();
        runtime::logging::init();
        crate::boot_timebase::probe_and_init_timebase(boot_arg1);
        info!("log test pass!");
        runtime::heap_allocator::init();
        #[cfg(feature = "heap-stress-boot")]
        runtime::heap_allocator::heap_fragmentation_stress_report(20_000);
        use alloc::vec;
        let vec_test = vec![0; 10];
        debug!("vec_test = {:?}", vec_test);

        platform::arch::init();

        // 须在 MM 安装 satp 之前注册 trap 路由：satp 切换后定时器/页故障会立即进 trap。
        task::init();
        crate::trap_handler::init();

        // ===== 内核态自检：MM / FrameAllocator / Sv39 =====
        unsafe extern "C" {
            fn kernel_end();
        }
        // 与 DTB `/memory` 或 `wateros-base-config::QEMU_VIRT_PHYS_RAM_END` 对齐（如
        // QEMU `-m 1G` → 0xC000_0000）
        let memory_end = driver::physical_ram_end_exclusive();
        const PAGE_SIZE : usize = 4096;
        #[inline]
        const fn align_up(v : usize, align : usize) -> usize { (v + align - 1) & !(align - 1) }
        #[inline]
        const fn align_down(v : usize, align : usize) -> usize { v & !(align - 1) }
        let alloc_end = if boot_arg1 >= 0x8000_0000 && boot_arg1 < memory_end {
            let reserved = align_down(boot_arg1, PAGE_SIZE);
            info!("[self-test] reserve DTB/top RAM for firmware data: dtb={:#x} alloc_end={:#x} \
                   memory_end={:#x}",
                  boot_arg1, reserved, memory_end);
            reserved
        } else {
            memory_end
        };
        let start_ppn = align_up(kernel_end as *const () as usize,
                                 PAGE_SIZE) /
                        PAGE_SIZE;
        let end_ppn = alloc_end / PAGE_SIZE;
        info!("[self-test] frame range ppn=[{:#x},{:#x})",
              start_ppn, end_ppn);
        mm::test_with_range(base::addr::BasePPN { val : start_ppn },
                            base::addr::BasePPN { val : end_ppn });
        mm::kernel_mm::init(start_ppn, end_ppn, memory_end);
        info!("[self-test] mm self-test done");

        // 设备驱动扫描与根文件系统挂载自检。
        let driver_boot = driver::active_impl::init_after_boot();
        if let Err(ref err) = driver_boot {
            warn!("[self-test] driver init failed: {:?}",
                  err);
        } else {
            info!("[self-test] driver init done");
            match driver::network::stack::init([10, 0, 2, 15], [10, 0, 2, 2]) {
                Ok(()) => {
                    task::spawn_kernel_task(network_poller_task, 0);
                    // 同步烟测：在调度器启动前验证核心 API
                    crate::self_tests::network::run_sync_smoke();
                }
                Err(e) => {
                    warn!("[self-test] network stack init skipped: {}",
                          e);
                }
            }
            fs::init();
            // ----- 用户态 bring-up 总线：RW 挂载根卷 + 用户 ELF spawn（见
            // `user_bringup_bus`） ----- 注意：`run()` 内 `spawn_user_task_*`
            // 只入队；用户测程的 `ecall` 在下方 `run_first_task()`
            // 之后才会出现。
            crate::user_bringup_bus::run();
            crate::self_tests::task::spawn_all();
            crate::self_tests::network::spawn_all();
            fs::test();
            #[cfg(feature = "vfs-bridge")]
            {
                vfs::test();
            }
        }
        if driver_boot.is_err() {
            // crate::self_tests::task::spawn_all();  // 禁用
        }

        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        klog::post_init_hello();
        info!("[task-selftest] starting first task");
        task::run_first_task()
    }
}

#[cfg(feature = "qemu-loongarch64-virt")]
mod qemu_loongarch64_virt {
    //! QEMU LoongArch `virt` 板级的最小 bring-up：与
    //! `impl-qemu-loongarch64-virt` 的 `_start.S`
    //! 链接后进入 [`kernel_main`]，初始化 runtime/任务、MM/FS/ELF bring-up
    //! 与两个内核忙等任务， 再开定时器中断并进入调度。
    use core::arch::global_asm;
    use core::include_str;
    use runtime::logging::*;

    global_asm!(include_str!("../components/wateros-platform/platform-impl/\
                              impl-qemu-loongarch64-virt/src/asm/_start.S"));

    /// 网络协议栈轮询任务：周期性驱动 smoltcp 收发包，永久运行。
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

    /// 固件/引导移交后的内核 C 入口；完成 MM bring-up、驱动、FS、VFS 与用户 ELF
    /// 装载后进入调度。
    ///
    /// `$r4` = argc, `$r5` = argv, `$r6` = envp（部分固件在此传递 FDT 指针）。
    #[unsafe(no_mangle)]
    pub fn kernel_main(_argc : usize, _argv : usize, envp : usize) -> ! {
        runtime::console::show_logo();
        klog::init();
        runtime::logging::init();
        runtime::heap_allocator::init();
        platform::arch::init();
        info!("[loongarch64] boot smoke ok");
        driver::init_when_boot(envp);
        crate::boot_timebase::probe_and_init_timebase(envp);

        // 必须在 MM 初始化之前注册 trap handler：页表激活后的探针访问可能触发页错误。
        task::init();
        crate::trap_handler::init();

        // 关闭固件可能已开启的 MMU，确认为后续页表构建与 PCI ECAM/MMIO 访问使用直接寻址。
        platform::arch::paging::init_paging_disable_mmu();

        // ===== 内核态自检：MM / FrameAllocator / LoongArch64 三级页表 =====
        unsafe extern "C" {
            fn kernel_end();
        }
        const PAGE_SIZE : usize = mm::api::addr::PAGE_SIZE;
        #[inline]
        const fn align_up(v : usize, align : usize) -> usize { (v + align - 1) & !(align - 1) }
        #[inline]
        const fn align_down(v : usize, align : usize) -> usize { v & !(align - 1) }

        let start_ppn = align_up(kernel_end as *const () as usize,
                                 PAGE_SIZE) /
                        PAGE_SIZE;
        let memory_end = driver::physical_ram_end_exclusive();
        let alloc_end = if envp >= 0x8000_0000 && envp < memory_end {
            let reserved = align_down(envp, PAGE_SIZE);
            info!("[loongarch64][self-test] reserve DTB/top RAM for firmware data: envp={:#x} \
                   alloc_end={:#x} memory_end={:#x}",
                  envp, reserved, memory_end);
            reserved
        } else {
            memory_end
        };
        let end_ppn = alloc_end / PAGE_SIZE;
        let usable_end_ppn = end_ppn.min(0x1_0000_0000usize / PAGE_SIZE);
        info!("[loongarch64][self-test] frame range ppn=[{:#x},{:#x}) detected_end_ppn={:#x} \
               alloc_end={:#x} ram_end={:#x}",
              start_ppn, usable_end_ppn, end_ppn, alloc_end, memory_end);
        mm::test_with_range(base::addr::BasePPN { val : start_ppn },
                            base::addr::BasePPN { val : usable_end_ppn });
        info!("[loongarch64][self-test] mm self-test done");

        mm::kernel_mm::init(start_ppn, usable_end_ppn, memory_end);
        info!("[loongarch64][self-test] kernel paging enabled");

        let driver_boot = driver::active_impl::init_after_boot();
        if let Err(ref err) = driver_boot {
            warn!("[loongarch64][self-test] driver init failed: {:?}",
                  err);
        } else {
            info!("[loongarch64][self-test] driver init done");
            match driver::network::stack::init([10, 0, 2, 15], [10, 0, 2, 2]) {
                Ok(()) => {
                    task::spawn_kernel_task(network_poller_task, 0);
                    crate::self_tests::network::run_sync_smoke();
                }
                Err(e) => {
                    warn!("[loongarch64][self-test] network stack init skipped: {}",
                          e);
                }
            }
            fs::init();
            crate::user_bringup_bus::run();
            crate::self_tests::network::spawn_all();
            fs::test();
            #[cfg(feature = "vfs-bridge")]
            {
                vfs::test();
            }
        }
        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        klog::post_init_hello();
        info!("[loongarch64][task] starting first task");
        task::run_first_task()
    }
}
