//! 全局内核 Sv39 页表：根表与中间表由帧分配器分配，运行期 `Box::leak` 永不
//! `Drop`，避免 `satp` 悬空。
//!
//! ## trap 与映射语义
//!
//! [`init`] 在 **S 态** 安装 `satp` 后映射 QEMU virt **RAM
//! 恒等区**（`0x8000_0000` 起，`R|W|X`）与 **MMIO**（`R|W`），保证内核、trap
//! 入口、设备 MMIO 访问在同一套映射下有效；不包含 `U`
//! 位，用户态须使用独立用户页表或后续 `protect`/`map_identity_range_user`
//! 等路径。
//!
//! ## 页大小与 PPN
//!
//! 恒等映射使用 `vpn == ppn`（[`VirtPageNum::to_phys_page_identity`]），隐含
//! **4 KiB** 页与物理 RAM 布局一致；`ram_end_exclusive` 须与
//! DTB/固件约定对齐（上界不包含）。

extern crate alloc;

use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::MmError;
use api_v0::perm::PagePerm;
use frame_alloctor::frame_alloc_result;
use wateros_base::sync::{BootOnceCell, MultiprocessorSafeCell};

use crate::pagetable::Sv39AddressSpace;

struct KernelAddressSpaceCell {
    inner : MultiprocessorSafeCell<Sv39AddressSpace>,
}

static KERNEL_ASPACE : BootOnceCell<KernelAddressSpaceCell> = BootOnceCell::new();

/// 恒等映射物理 RAM 上界（不包含）；由 `kernel_mm::init` 写入，供 ELF
/// 装载等路径读取。
static PHYS_RAM_END_EXCL : AtomicUsize = AtomicUsize::new(0);


// `Acquire` 与 `init` 末尾 `Release` 配对；仅在 `init` 完成后合法调用。
#[inline]
fn with_kernel_aspace<R>(f: impl FnOnce(&mut Sv39AddressSpace) -> R) -> R {
    let cell = KERNEL_ASPACE.get().expect("kernel_mm: not initialized");
    let mut guard = cell.inner.exclusive_access();
    f(&mut guard)
}

/// 安装后的内核 `satp`（与 `AddressSpaceHandle::from_raw` 配合使用）。
///
/// 缓存由 `mm-api` 的 `kernel_satp` 模块维护，`init` 末尾自动写入。
#[inline]
pub fn kernel_satp() -> usize { api_v0::kernel_satp::get() }

/// QEMU virt RAM 恒等映射（S 态、无 `U`），保证 trap / 调度代码可执行。
///
/// `ram_end_exclusive` 为物理 RAM 上界（不包含），应与 DTB `/memory` 或
/// bring-up 约定一致。
pub fn init(dtb_pa : usize, ram_end_exclusive : usize) {
    assert!(ram_end_exclusive > 0x8000_0000,
            "kernel_mm: ram_end_exclusive must be above RAM base");
    PHYS_RAM_END_EXCL.store(ram_end_exclusive, Ordering::Release);

    // 从 kernel_end 到 DTB `/memory` 上界都属于帧池；DTB 自身仅作为一个小的
    // reserved region 排除，不能再把 DTB 的放置地址误当成 RAM 终点。QEMU 9.2.1
    // 会把 16 GiB machine 的 DTB 放在约 3 GiB，旧逻辑因此错误丢弃后方 13 GiB。
    // 用 inline asm 取 kernel_end 符号地址（避免 extern static 指针语法歧义）
    let kernel_end_addr : usize;
    unsafe {
        core::arch::asm!("la {}, kernel_end", out(reg) kernel_end_addr);
    }
    let start_ppn = (kernel_end_addr + PAGE_SIZE - 1) / PAGE_SIZE;
    let end_ppn = ram_end_exclusive / PAGE_SIZE;
    let (reserved_start_ppn, reserved_end_ppn) = dtb_reserved_ppns(dtb_pa,
                                                                   ram_end_exclusive)
        .unwrap_or((start_ppn, start_ppn));

    frame_alloctor::init_frame_allocator_with_reserved(PhysPageNum(start_ppn),
                                                       PhysPageNum(end_ppn),
                                                       PhysPageNum(reserved_start_ppn),
                                                       PhysPageNum(reserved_end_ppn));

    let mut aspace = Sv39AddressSpace::new_kernel()
        .expect("kernel_mm: Sv39AddressSpace::new_kernel failed");

    let map_identity = |aspace : &mut Sv39AddressSpace,
                        start : usize,
                        end : usize,
                        perm : PagePerm,
                        what : &str| {
        let lo = VirtAddr(start).floor_page();
        let hi = VirtAddr(end).ceil_page();
        for vpn_raw in lo.0..hi.0 {
            let vpn = VirtPageNum(vpn_raw);
            let ppn = vpn.to_phys_page_identity();
            aspace.map_page_to_ppn(vpn, ppn, perm)
                  .unwrap_or_else(|e| {
                      panic!("kernel_mm: identity map {} [{:#x},{:#x}): {:?}",
                             what, start, end, e)
                  });
        }
    };

    map_identity(&mut aspace,
                 0x8000_0000,
                 ram_end_exclusive,
                 PagePerm::R | PagePerm::W | PagePerm::X,
                 "RAM");

    // 访问 virtio / UART 等 MMIO（如 0x1000_8000）必须映射；与 `-m` 无关。
    map_identity(&mut aspace,
                 wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_START,
                 wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_END,
                 PagePerm::R | PagePerm::W,
                 "MMIO");

    // Goldfish RTC 位于 0x0010_1000，不在 UART/VirtIO 的常规 MMIO 窗口内。
    map_identity(&mut aspace,
                 wateros_base_config::mm::QEMU_VIRT_RTC_PHYS_START,
                 wateros_base_config::mm::QEMU_VIRT_RTC_PHYS_END,
                 PagePerm::R | PagePerm::W,
                 "RTC MMIO");

    // 选一枚位于帧池内的物理页做 satp 切换后的翻译与内存一致性探针（与 RAM
    // 恒等区无重叠的任意 VA）。
    assert!(start_ppn + 16 < end_ppn,
            "kernel_mm: probe ppn out of range");
    let probe_ppn = PhysPageNum(start_ppn + 16);
    let probe_va = VirtAddr(0x4000_0000usize + 0x2A0);
    let probe_vpn = probe_va.floor_page();
    aspace.map_page_to_ppn(probe_vpn,
                           probe_ppn,
                           PagePerm::R | PagePerm::W)
          .expect("kernel_mm: map probe page");

    let satp_target = aspace.kernel_satp_value();
    #[cfg(all(feature = "impl-riscv64", target_arch = "riscv64"))]
    platform::arch::trap::set_kernel_trap_satp(satp_target);
    runtime::logging::trace!("[kernel-mm] identity map RAM [0x80000000,{:#x}) MMIO [{:#x},{:#x}) \
                              satp target={:#x}",
                             ram_end_exclusive,
                             wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_START,
                             wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_END,
                             satp_target);
    platform::arch::paging::activate_address_space_token_and_flush(satp_target);
    assert_eq!(platform::arch::paging::active_address_space_token(),
               satp_target,
               "kernel_mm: satp mismatch");
    let asid_bits = platform::arch::paging::initialize_address_space_ids();
    crate::asid::initialize(asid_bits);
    runtime::logging::trace!("[kernel-mm] RISC-V ASID bits={} enabled={}",
                             asid_bits,
                             asid_bits != 0);

    let translated = aspace.translate_addr(probe_va)
                           .expect("kernel_mm: translate")
                           .expect("kernel_mm: probe should translate");
    assert_eq!(translated.0,
               probe_ppn.0 * PAGE_SIZE + probe_va.page_offset(),
               "kernel_mm: translate_addr mismatch");

    let probe_ptr = probe_va.0 as *mut u64;
    unsafe {
        probe_ptr.write_volatile(0x1122_3344_5566_7788);
    }
    let probe_pa = PhysAddr(probe_ppn.0 * PAGE_SIZE + probe_va.page_offset());
    let phys_ptr = probe_pa.0 as *const u64;
    let observed = unsafe { phys_ptr.read_volatile() };
    assert_eq!(observed, 0x1122_3344_5566_7788);
    runtime::logging::trace!("[kernel-mm] paging probe ok va={:#x} -> pa={:#x}",
                             probe_va.0,
                             probe_pa.0);

    KERNEL_ASPACE.init(KernelAddressSpaceCell { inner : MultiprocessorSafeCell::new(aspace) })
                 .expect("kernel_mm: duplicate initialization");
    api_v0::kernel_satp::set(satp_target);
    api_v0::user_aspace_lifecycle::register_drop_user_aspace_hook(crate::kernel_mm_impl::drop_user_aspace);
    api_v0::user_aspace_lifecycle::register_aspace_cpu_hooks(crate::user_aspace::mark_active,
                                                              crate::user_aspace::mark_inactive);
    api_v0::user_mapping::register_snapshot_user_mappings_hook(
        crate::user_aspace::snapshot_user_mappings,
    );
}

/// 返回 DTB 实际 blob 覆盖的页区间；无效/不在 RAM 中时不建立保留区。
fn dtb_reserved_ppns(dtb_pa : usize, ram_end_exclusive : usize) -> Option<(usize, usize)> {
    if dtb_pa < 0x8000_0000 || dtb_pa >= ram_end_exclusive {
        return None;
    }
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) }.ok()?;
    let dtb_end = dtb_pa.checked_add(fdt.total_size())?
                        .min(ram_end_exclusive);
    let start_ppn = dtb_pa / PAGE_SIZE;
    let end_ppn = dtb_end.checked_add(PAGE_SIZE - 1)? / PAGE_SIZE;
    (end_ppn > start_ppn).then_some((start_ppn, end_ppn))
}

/// 将 `[va_start, va_end)` 内每一虚拟页映射到 **恒等物理页**，并设置
/// `perm`（通常含 `U` 供用户态访问）。
pub fn map_identity_range_user(start : VirtAddr, end : VirtAddr, perm : PagePerm) {
    assert!(start.0 < end.0,
            "kernel_mm: empty range");
    let mut vpn = start.floor_page();
    let vpn_end = end.ceil_page();
    with_kernel_aspace(|a| while vpn.0 < vpn_end.0 {
        let ppn = vpn.to_phys_page_identity();
        match a.map_page_to_ppn(vpn, ppn, perm) {
            Ok(()) | Err(MmError::AlreadyMapped) => {}
            Err(e) => panic!("kernel_mm: map {:?} -> {:?}: {:?}", vpn, ppn, e),
        }
        vpn = VirtPageNum(vpn.0 + 1);
    });
}

/// 为已恒等映射的内核文本页增加 `U`，供用户态执行（如 stage4
/// 入口落在内核镜像内）。
pub fn ensure_user_execute_for_kernel_va(va : usize) {
    let vpn = VirtAddr(va).floor_page();
    with_kernel_aspace(|a| a.protect_page(vpn,
                              PagePerm::R | PagePerm::X | PagePerm::U))
        .expect("kernel_mm: protect_page for user exec");
}

/// 为一段虚拟地址分配匿名帧并映射（用于用户栈等）。
pub fn map_anon_range_user(start : VirtAddr, end : VirtAddr, perm : PagePerm) {
    assert!(start.0 < end.0,
            "kernel_mm: empty anon range");
    let mut vpn = start.floor_page();
    let vpn_end = end.ceil_page();
    with_kernel_aspace(|a| while vpn.0 < vpn_end.0 {
        if a.translate_addr(vpn.start_addr())
            .ok()
            .flatten()
            .is_none()
        {
            let ppn = frame_alloc_result().expect("kernel_mm: frame oom for anon map");
            match a.map_page_to_ppn(vpn, ppn, perm) {
                Ok(()) => {}
                Err(MmError::AlreadyMapped) => {}
                Err(e) => panic!("kernel_mm: anon map {:?}: {:?}", vpn, e),
            }
        }
        vpn = VirtPageNum(vpn.0 + 1);
    });
}
