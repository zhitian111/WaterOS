//! 全局内核 Sv39 页表：根表与中间表由帧分配器分配，运行期 `Box::leak` 永不 `Drop`，避免 `satp` 悬空。
//!
//! ## trap 与映射语义
//!
//! [`init`] 在 **S 态** 安装 `satp` 后映射 QEMU virt **RAM 恒等区**（`0x8000_0000` 起，`R|W|X`）与 **MMIO**（`R|W`），保证内核、trap 入口、设备 MMIO 访问在同一套映射下有效；不包含 `U` 位，用户态须使用独立用户页表或后续 `protect`/`map_identity_range_user` 等路径。
//!
//! ## 页大小与 PPN
//!
//! 恒等映射使用 `vpn == ppn`（[`VirtPageNum::to_phys_page_identity`]），隐含 **4 KiB** 页与物理 RAM 布局一致；`ram_end_exclusive` 须与 DTB/固件约定对齐（上界不包含）。

extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use api_v0::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::MmError;
use api_v0::perm::PagePerm;
use frame_alloctor::frame_alloc_result;

use crate::pagetable::Sv39AddressSpace;

static KERNEL_ASPACE: AtomicPtr<Sv39AddressSpace> = AtomicPtr::new(core::ptr::null_mut());

/// 恒等映射物理 RAM 上界（不包含）；由 `kernel_mm::init` 写入，供 ELF 装载等路径读取。
static PHYS_RAM_END_EXCL: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub(crate) fn phys_ram_end_exclusive() -> usize {
    let v = PHYS_RAM_END_EXCL.load(Ordering::Acquire);
    if v != 0 {
        v
    } else {
        wateros_base_config::mm::QEMU_VIRT_PHYS_RAM_END
    }
}

// `Acquire` 与 `init` 末尾 `Release` 配对；仅在 `init` 完成后合法调用。
#[inline]
fn aspace_mut() -> &'static mut Sv39AddressSpace {
    let p = KERNEL_ASPACE.load(Ordering::Acquire);
    assert!(!p.is_null(), "kernel_mm: not initialized");
    unsafe { &mut *p }
}

/// 安装后的内核 `satp`（与 `AddressSpaceHandle::from_raw` 配合使用）。
#[inline]
pub fn kernel_satp() -> usize {
    aspace_mut().satp_value()
}

/// QEMU virt RAM 恒等映射（S 态、无 `U`），保证 trap / 调度代码可执行。
///
/// `ram_end_exclusive` 为物理 RAM 上界（不包含），应与 DTB `/memory` 或 bring-up 约定一致。
pub fn init(start_ppn: usize, end_ppn: usize, ram_end_exclusive: usize) {
    assert!(
        ram_end_exclusive > 0x8000_0000,
        "kernel_mm: ram_end_exclusive must be above RAM base"
    );
    PHYS_RAM_END_EXCL.store(ram_end_exclusive, Ordering::Release);

    let mut aspace = Sv39AddressSpace::new().expect("kernel_mm: Sv39AddressSpace::new failed");

    let map_identity = |aspace: &mut Sv39AddressSpace,
                        start: usize,
                        end: usize,
                        perm: PagePerm,
                        what: &str| {
        let lo = VirtAddr(start).floor_page();
        let hi = VirtAddr(end).ceil_page();
        for vpn_raw in lo.0..hi.0 {
            let vpn = VirtPageNum(vpn_raw);
            let ppn = vpn.to_phys_page_identity();
            aspace
                .map_page_to_ppn(vpn, ppn, perm)
                .unwrap_or_else(|e| panic!("kernel_mm: identity map {} [{:#x},{:#x}): {:?}", what, start, end, e));
        }
    };

    map_identity(
        &mut aspace,
        0x8000_0000,
        ram_end_exclusive,
        PagePerm::R | PagePerm::W | PagePerm::X,
        "RAM",
    );

    // 访问 virtio / UART 等 MMIO（如 0x1000_8000）必须映射；与 `-m` 无关。
    map_identity(
        &mut aspace,
        wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_START,
        wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_END,
        PagePerm::R | PagePerm::W,
        "MMIO",
    );

    // 选一枚位于帧池内的物理页做 satp 切换后的翻译与内存一致性探针（与 RAM 恒等区无重叠的任意 VA）。
    assert!(start_ppn + 16 < end_ppn, "kernel_mm: probe ppn out of range");
    let probe_ppn = PhysPageNum(start_ppn + 16);
    let probe_va = VirtAddr(0x4000_0000usize + 0x2A0);
    let probe_vpn = probe_va.floor_page();
    aspace
        .map_page_to_ppn(probe_vpn, probe_ppn, PagePerm::R | PagePerm::W)
        .expect("kernel_mm: map probe page");

    let satp_target = aspace.satp_value();
    runtime::logging::info!(
        "[kernel-mm] identity map RAM [0x80000000,{:#x}) MMIO [{:#x},{:#x}) satp target={:#x}",
        ram_end_exclusive,
        wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_START,
        wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_END,
        satp_target
    );
    platform::arch::paging::activate_address_space_token_and_flush(satp_target);
    assert_eq!(
        platform::arch::paging::active_address_space_token(),
        satp_target,
        "kernel_mm: satp mismatch"
    );

    let translated = aspace
        .translate_addr(probe_va)
        .expect("kernel_mm: translate")
        .expect("kernel_mm: probe should translate");
    assert_eq!(
        translated.0,
        probe_ppn.0 * PAGE_SIZE + probe_va.page_offset(),
        "kernel_mm: translate_addr mismatch"
    );

    let probe_ptr = probe_va.0 as *mut u64;
    unsafe {
        probe_ptr.write_volatile(0x1122_3344_5566_7788);
    }
    let probe_pa = PhysAddr(probe_ppn.0 * PAGE_SIZE + probe_va.page_offset());
    let phys_ptr = probe_pa.0 as *const u64;
    let observed = unsafe { phys_ptr.read_volatile() };
    assert_eq!(observed, 0x1122_3344_5566_7788);
    runtime::logging::info!(
        "[kernel-mm] paging probe ok va={:#x} -> pa={:#x}",
        probe_va.0,
        probe_pa.0
    );

    let leaked = Box::leak(Box::new(aspace));
    KERNEL_ASPACE.store(leaked as *mut Sv39AddressSpace, Ordering::Release);
}

/// 将 `[va_start, va_end)` 内每一虚拟页映射到 **恒等物理页**，并设置 `perm`（通常含 `U` 供用户态访问）。
pub fn map_identity_range_user(start: VirtAddr, end: VirtAddr, perm: PagePerm) {
    assert!(start.0 < end.0, "kernel_mm: empty range");
    let mut vpn = start.floor_page();
    let vpn_end = end.ceil_page();
    let a = aspace_mut();
    while vpn.0 < vpn_end.0 {
        let ppn = vpn.to_phys_page_identity();
        match a.map_page_to_ppn(vpn, ppn, perm) {
            Ok(()) => {}
            Err(MmError::AlreadyMapped) => {}
            Err(e) => panic!("kernel_mm: map {:?} -> {:?}: {:?}", vpn, ppn, e),
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }
}

/// 为已恒等映射的内核文本页增加 `U`，供用户态执行（如 stage4 入口落在内核镜像内）。
pub fn ensure_user_execute_for_kernel_va(va: usize) {
    let vpn = VirtAddr(va).floor_page();
    aspace_mut()
        .protect_page(vpn, PagePerm::R | PagePerm::X | PagePerm::U)
        .expect("kernel_mm: protect_page for user exec");
}

/// 为一段虚拟地址分配匿名帧并映射（用于用户栈等）。
pub fn map_anon_range_user(start: VirtAddr, end: VirtAddr, perm: PagePerm) {
    assert!(start.0 < end.0, "kernel_mm: empty anon range");
    let mut vpn = start.floor_page();
    let vpn_end = end.ceil_page();
    let a = aspace_mut();
    while vpn.0 < vpn_end.0 {
        if a.translate_addr(vpn.start_addr()).ok().flatten().is_none() {
            let ppn = frame_alloc_result().expect("kernel_mm: frame oom for anon map");
            match a.map_page_to_ppn(vpn, ppn, perm) {
                Ok(()) => {}
                Err(MmError::AlreadyMapped) => {}
                Err(e) => panic!("kernel_mm: anon map {:?}: {:?}", vpn, e),
            }
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }
}
