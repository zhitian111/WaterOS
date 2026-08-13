#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

#[cfg(all(feature = "pre", feature = "final_online"))]
compile_error!("features `pre` and `final_online` are mutually exclusive");
#[cfg(not(any(feature = "pre", feature = "final_online")))]
compile_error!("select one competition stage feature: `pre` or `final_online`");
#[cfg(all(feature = "user-graphics", feature = "gui"))]
compile_error!("features `user-graphics` and `gui`/`display-demo` are mutually exclusive");

extern crate alloc;

use klog as _;
#[cfg(feature = "gui")]
use runtime::logging::info;
use runtime::logging::warn;
#[cfg(any(feature = "qemu-riscv64-opensbi", feature = "qemu-loongarch64-virt"))]
use syscall as _;

mod boot_timebase;
#[cfg(feature = "dashboard-debug")]
mod dashboard;
#[cfg(feature = "gdb-fault-injection")]
mod debug_fault;
#[cfg(feature = "stall-debug")]
mod stall_debug;
mod trap_handler;
mod user_bringup_bus;
mod user_bringup_busybox;
mod user_bringup_common;
mod user_bringup_ltp_exclusions;
mod user_bringup_mm;
mod user_bringup_posix_fs;
mod user_bringup_root_layout;
mod user_operator;

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
        // NETWORK_STACK is a cross-CPU spin lock. Keep this kernel task from
        // being switched out while it owns the lock: syscall callers enter
        // with interrupts disabled and otherwise cannot yield while spinning.
        let interrupt_state =
            platform::arch::interrupt::read_global_interrupt_state().expect("read interrupt \
                                                                             state for network \
                                                                             poll");
        platform::arch::interrupt::disable_global_interrupt().expect("disable interrupts for \
                                                                      network poll");
        match platform::timer::now_duration() {
            Ok(now) => {
                let millis = now.as_millis()
                                .min((i64::MAX / 1000) as u128) as i64;
                network::stack::poll_at_millis(millis);
            }
            Err(_) => network::stack::poll(),
        }
        network::stack::poll_socket_events();
        platform::arch::interrupt::restore_global_interrupt_state(interrupt_state)
            .expect("restore interrupts after network poll");
        task::sleep_for_ticks(1);
    }
}

/// GUI 常驻任务：处理输入、更新默认动画并仅在 dirty 时提交画面。
#[cfg(feature = "gui")]
extern "C" fn gui_refresh_task(_arg : usize) -> ! {
    let mut frame = 0u64;
    loop {
        let _ = gui::push_input(gui::InputEvent::Tick(frame));
        let _ = gui::update_default_desktop(frame);
        let _ = gui::render_if_dirty();

        // 默认桌面拥有这些事件；未来由专用 GUI 服务任务把事件转交应用。
        while let Ok(Some(event)) = gui::poll_event() {
            match event.kind {
                gui::GuiEventKind::CloseRequested => {
                    let _ = gui::remove_window(event.window);
                }
                gui::GuiEventKind::Clicked if event.widget == Some(gui::ACTION_BUTTON) => {
                    let _ = gui::set_label_text(gui::MAIN_WINDOW,
                                                gui::STATUS_LABEL,
                                                "Self-check event delivered successfully");
                }
                gui::GuiEventKind::Submitted => {
                    let _ = gui::set_label_text(gui::MAIN_WINDOW,
                                                gui::STATUS_LABEL,
                                                "Text input submitted");
                }
                _ => {}
            }
        }
        frame = frame.wrapping_add(1);
        task::sleep_for_ticks(2);
    }
}

/// 驱动 → 网络 → FS → 用户态 bring-up。两 board 模块共用。
fn bringup_driver_and_user() {
    match driver::machine().init_after_boot() {
        Err(ref err) => warn!("driver init failed: {:?}", err),
        Ok(()) => {
            #[cfg(feature = "user-graphics")]
            {
                let has_input = vfs::initialize_user_graphics_devices();
                if has_input {
                    task::spawn_kernel_task(vfs::user_graphics_input_worker, 0);
                }
                warn!("[user-graphics] fbdev/evdev ready input_worker={}", has_input);
            }
            #[cfg(feature = "gui")]
            match (|| -> gui::GuiResult<()> {
                gui::initialize()?;
                gui::install_default_desktop()?;
                let _ = gui::render()?;
                Ok(())
            })() {
                Ok(()) => {
                    task::spawn_kernel_task(gui_refresh_task, 0);
                    info!("[gui] wateros-gui desktop and refresh task ready");
                }
                Err(error) => warn!("[gui] initialization skipped: {:?}", error),
            }
            match driver::machine().realtime_ns() {
                Ok(Some(ns)) => {
                    if platform::wall_clock::set_realtime_ns(u128::from(ns)).is_err() {
                        warn!("[boot] failed to initialize CLOCK_REALTIME from machine RTC");
                    }
                }
                Ok(None) => {}
                Err(err) => warn!("[boot] machine RTC unavailable: {:?}",
                                  err),
            }
            match network::stack::init(network::NetworkConfig { address : [10, 0, 2, 15],
                                                                prefix_len : 24,
                                                                gateway : [10, 0, 2, 2] })
            {
                Ok(()) => {
                    task::spawn_kernel_task(network_poller_task, 0);
                }
                Err(e) => warn!("network stack init skipped: {}", e),
            }
            fs::init();
            crate::user_bringup_bus::run();
        }
    }
}

#[cfg(feature = "qemu-riscv64-opensbi")]
mod qemu_riscv64_opensbi {
    use crate::bringup_driver_and_user;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use runtime::logging::*;
    /// Firmware-selected boot hart. `usize::MAX` means no hart entered yet.
    static BSP_HART : AtomicUsize = AtomicUsize::new(usize::MAX);
    /// BSP 完成初始化后置 true，AP 自旋等待此标志。
    static AP_BOOT_READY : AtomicBool = AtomicBool::new(false);

    unsafe extern "C" {
        fn __wateros_arch_boot();
    }

    fn start_secondary_harts(boot_cpu : task::CpuId, dtb_pa : usize) -> base::cpu::CpuMask {
        let entry = __wateros_arch_boot as *const () as usize;
        let mut requested = base::cpu::CpuMask::EMPTY;
        for raw in 0..base_config::task::MAX_CPUS {
            let cpu = task::CpuId::from_raw(raw);
            if cpu == boot_cpu {
                continue;
            }
            info!("[smp] hart_start cpu={} entry={:#x} opaque={:#x}",
                  raw, entry, dtb_pa);
            match platform::smp::start_cpu(cpu, entry, dtb_pa) {
                Ok(()) | Err(platform::smp::PlatformSmpError::AlreadyAvailable) => {
                    requested.insert(cpu);
                    let status = platform::smp::cpu_status(cpu);
                    info!("[smp] hart_start accepted cpu={} status={:?}",
                          raw, status);
                }
                // A smaller QEMU `-smp` simply has no hart at this index.
                Err(platform::smp::PlatformSmpError::InvalidCpu) => break,
                Err(error) => panic!("[smp] cannot start cpu={}: {:?}; use an OpenSBI firmware \
                                      with HSM",
                                     raw, error),
            }
        }
        requested
    }

    /// Do not enter userspace with a partially initialized SMP configuration.
    /// A finite spin wait is deliberate: the timer and scheduler are not yet
    /// running on the BSP, so a timeout cannot rely on kernel timekeeping.
    fn wait_for_secondary_online(requested : base::cpu::CpuMask) {
        const ONLINE_WAIT_SPINS : usize = 100_000_000;
        for _ in 0..ONLINE_WAIT_SPINS {
            let online = task::online_cpu_mask();
            if online.bits() & requested.bits() == requested.bits() {
                return;
            }
            core::hint::spin_loop();
        }
        let online = task::online_cpu_mask();
        warn!("[smp] AP online timeout requested={:#x} online={:#x}",
              requested.bits(),
              online.bits());
        panic!("[smp] AP online timeout: requested={:#x}, online={:#x}",
               requested.bits(),
               online.bits());
    }
    /// AP 入口：BSP 初始化完成后被调用；局部初始化后加入调度。
    fn ap_main(cpu_id : task::CpuId) -> ! {
        warn!("[smp] AP entered Rust cpu={}",
              cpu_id.raw());
        platform::arch::cpu::init_current_cpu(cpu_id).expect("AP init current CPU");
        platform::arch::init();
        let _ = platform::smp::init_ipi();
        platform::arch::paging::activate_address_space_token_and_flush(mm::kernel_mm::kernel_satp());
        // 开 AP 定时器中断，使 idle 能被 tick 唤醒从而从全局就绪队列取任务
        platform::interrupt::enable_timer_interrupt().expect("AP enable timer interrupt");
        platform::arch::interrupt::enable_soft_interrupt();
        platform::timer::set_timer_after_ms(100).expect("AP set initial timer");
        task::set_cpu_online(cpu_id);
        platform::interrupt::enable_global_interrupt().expect("AP enable global interrupt");
        task::run_first_task()
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
        platform::arch::interrupt::disable_soft_interrupt();
    }

    #[unsafe(no_mangle)]
    pub fn wateros_kernel_main(cpu_raw : usize, dtb_pa : usize, _platform_arg1 : usize) -> ! {
        let cpu_id = task::CpuId::from_raw(cpu_raw);
        platform::arch::cpu::init_current_cpu(cpu_id).expect("init current CPU");
        mask_boot_interrupts();
        // OpenSBI supplies the boot hart.  Secondary harts can arrive either
        // directly from firmware or through the SBI HSM entry below.
        if BSP_HART.compare_exchange(usize::MAX,
                                     cpu_raw,
                                     Ordering::AcqRel,
                                     Ordering::Acquire)
                   .is_err()
        {
            wait_ap_boot_ready(cpu_id);
        }
        // BSP 初始化：驱动 → 日志 → timebase → 堆 → arch → 任务 → trap
        platform::init_when_boot(dtb_pa);
        runtime::init_console();
        runtime::showlogo();
        klog::init();
        runtime::logging::init();
        crate::boot_timebase::probe_and_init_timebase(dtb_pa);
        runtime::heap_allocator::init();
        platform::arch::init();
        task::init();
        task::set_timekeeper_cpu(cpu_id);
        #[cfg(feature = "dashboard-debug")]
        crate::dashboard::init();
        crate::trap_handler::init();
        // MM 初始化
        let memory_end = platform::physical_ram_end_exclusive();
        mm::kernel_mm::init(dtb_pa, memory_end);
        AP_BOOT_READY.store(true, Ordering::Release);

        let requested_aps = start_secondary_harts(cpu_id, dtb_pa);
        wait_for_secondary_online(requested_aps);

        bringup_driver_and_user();
        #[cfg(feature = "stall-debug")]
        crate::stall_debug::start();
        #[cfg(feature = "dashboard-debug")]
        crate::dashboard::start();
        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::arch::interrupt::enable_soft_interrupt();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        task::run_first_task()
    }
}

#[cfg(feature = "qemu-loongarch64-virt")]
mod qemu_loongarch64_virt {
    use crate::bringup_driver_and_user;
    use core::sync::atomic::{AtomicBool, Ordering};
    use runtime::logging::*;

    static BSP_CLAIMED : AtomicBool = AtomicBool::new(false);
    static AP_BOOT_READY : AtomicBool = AtomicBool::new(false);

    unsafe extern "C" {
        fn _start();
    }

    fn start_secondary_cpus(boot_cpu : task::CpuId) -> base::cpu::CpuMask {
        // QEMU 的 AP 固件不会为 mailbox 入口准备 a0；必须从平台 `_start`
        // 进入，由它读取 CSR.CPUNUM，再跳转到通用 arch boot 入口。
        let entry = _start as *const () as usize;
        let configured = platform::smp::configured_cpu_mask();
        let mut requested = base::cpu::CpuMask::EMPTY;
        for raw in 0..base_config::task::MAX_CPUS {
            let cpu = task::CpuId::from_raw(raw);
            if cpu == boot_cpu || !configured.contains(cpu) {
                continue;
            }
            info!("[smp] starting LA cpu={} entry={:#x}",
                  raw, entry);
            match platform::smp::start_cpu(cpu, entry, 0) {
                Ok(()) | Err(platform::smp::PlatformSmpError::AlreadyAvailable) => {
                    requested.insert(cpu);
                }
                Err(platform::smp::PlatformSmpError::InvalidCpu) => break,
                Err(error) => panic!("[smp] cannot start LA cpu={}: {:?}",
                                     raw, error),
            }
        }
        requested
    }

    fn wait_for_secondary_online(requested : base::cpu::CpuMask) {
        const ONLINE_WAIT_SPINS : usize = 100_000_000;
        for _ in 0..ONLINE_WAIT_SPINS {
            let online = task::online_cpu_mask();
            if online.bits() & requested.bits() == requested.bits() {
                info!("[smp] all LA CPUs online mask={:#x}",
                      online.bits());
                return;
            }
            core::hint::spin_loop();
        }
        let online = task::online_cpu_mask();
        panic!("[smp] LA AP online timeout: requested={:#x}, online={:#x}",
               requested.bits(),
               online.bits());
    }

    fn ap_main(cpu_id : task::CpuId) -> ! {
        platform::arch::cpu::init_current_cpu(cpu_id).expect("AP init current CPU");
        platform::arch::init();
        let _ = platform::smp::init_ipi();
        platform::interrupt::enable_timer_interrupt().expect("AP enable timer interrupt");
        platform::arch::interrupt::enable_soft_interrupt();
        platform::timer::set_timer_after_ms(100).expect("AP set initial timer");
        task::set_cpu_online(cpu_id);
        platform::interrupt::enable_global_interrupt().expect("AP enable global interrupt");
        task::run_first_task()
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
        platform::arch::interrupt::disable_soft_interrupt();
    }

    #[unsafe(no_mangle)]
    pub fn wateros_kernel_main(cpu_raw : usize, _argc : usize, _argv : usize, _envp : usize) -> ! {
        let cpu_id = task::CpuId::from_raw(cpu_raw);
        mask_boot_interrupts();
        if BSP_CLAIMED.swap(true, Ordering::AcqRel) {
            wait_ap_boot_ready(cpu_id);
        }

        runtime::init_console();
        runtime::showlogo();
        klog::init();
        runtime::logging::init();
        runtime::heap_allocator::init();
        platform::arch::cpu::init_current_cpu(cpu_id).expect("BSP init current CPU");
        platform::arch::init();
        let _ = platform::smp::init_ipi();
        let dtb_pa = platform::active_impl::boot::device_tree_phys_addr();
        platform::init_when_boot(dtb_pa);
        let configured =
            platform::active_impl::smp::init_configured_cpu_mask(dtb_pa).expect("initialize \
                                                                                 LoongArch CPU \
                                                                                 topology from \
                                                                                 DTB");
        info!("[smp] LA configured CPU mask={:#x}",
              configured.bits());
        crate::boot_timebase::probe_and_init_timebase(dtb_pa);
        task::init();
        task::set_timekeeper_cpu(cpu_id);
        task::set_cpu_online(cpu_id);
        #[cfg(feature = "dashboard-debug")]
        crate::dashboard::init();
        crate::trap_handler::init();
        platform::arch::paging::init_paging_disable_mmu();

        let memory_end = platform::physical_ram_end_exclusive();
        mm::kernel_mm::init(dtb_pa, memory_end);

        AP_BOOT_READY.store(true, Ordering::Release);
        let requested_aps = start_secondary_cpus(cpu_id);
        wait_for_secondary_online(requested_aps);

        bringup_driver_and_user();
        #[cfg(feature = "stall-debug")]
        crate::stall_debug::start();
        #[cfg(feature = "dashboard-debug")]
        crate::dashboard::start();
        platform::interrupt::enable_timer_interrupt().unwrap();
        platform::arch::interrupt::enable_soft_interrupt();
        platform::timer::set_timer_after_ms(100).unwrap();
        platform::interrupt::enable_global_interrupt().unwrap();
        task::run_first_task()
    }
}
