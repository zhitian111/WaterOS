//! 全局内核 LoongArch64 页表：根表与中间表由帧分配器分配，运行期 `Box::leak`
//! 永不 `Drop`，避免 PGDL 悬空。
//!
//! ## trap 与映射语义
//!
//! [`init`] 在 **S 态**（内核 PLV0）安装 PGDL 后映射 QEMU virt **RAM
//! 恒等区**（`0x9000_0000` 起，`R|W|X`）与 **MMIO**（`R|W`），保证内核、trap
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

use crate::pagetable::LoongArch64AddressSpace;

struct KernelAddressSpaceCell {
    inner : MultiprocessorSafeCell<LoongArch64AddressSpace>,
}

static KERNEL_ASPACE : BootOnceCell<KernelAddressSpaceCell> = BootOnceCell::new();

/// 恒等映射物理 RAM 上界（不包含）；由 `kernel_mm::init` 写入，供 ELF
/// 装载等路径读取。
static PHYS_RAM_END_EXCL : AtomicUsize = AtomicUsize::new(0);

/// QEMU virt LoongArch64 RAM 基址（与 link.ld 一致）。
const LOONGARCH64_RAM_BASE : usize = 0x9000_0000;
const LOONGARCH64_LOW_MMIO_START : usize = 0x1000_0000;
const LOONGARCH64_LOW_MMIO_END : usize = 0x3000_0000;
const LOONGARCH64_PCI_MMIO_START : usize = 0x4000_0000;
const LOONGARCH64_PCI_MMIO_END : usize = 0x8000_0000;

#[inline]
pub(crate) fn phys_ram_end_exclusive() -> usize {
    let v = PHYS_RAM_END_EXCL.load(Ordering::Acquire);
    if v != 0 {
        v
    } else {
        // 回退值：仓库 QEMU LoongArch64 `virt -m 1G` 高 RAM 段上界。
        0xC000_0000
    }
}

// `Acquire` 与 `init` 末尾 `Release` 配对；仅在 `init` 完成后合法调用。
#[inline]
fn with_kernel_aspace<R>(f: impl FnOnce(&mut LoongArch64AddressSpace) -> R) -> R {
    let cell = KERNEL_ASPACE.get().expect("kernel_mm: not initialized");
    let mut guard = cell.inner.exclusive_access();
    f(&mut guard)
}

/// 安装后的内核 PGDL 值（与 `AddressSpaceHandle::from_raw` 配合使用）。
///
/// 缓存由 `mm-api` 的 `kernel_satp` 模块维护，`init` 末尾自动写入。
#[inline]
pub fn kernel_satp() -> usize { api_v0::kernel_satp::get() }

/// QEMU virt LoongArch64 RAM 恒等映射（S 态、无 `U`），保证 trap /
/// 调度代码可执行。
///
/// `ram_end_exclusive` 为物理 RAM 上界（不包含），应与 DTB `/memory` 或
/// bring-up 约定一致。
pub fn init(_dtb_pa : usize, ram_end_exclusive : usize) {
    assert!(ram_end_exclusive > LOONGARCH64_RAM_BASE,
            "kernel_mm: ram_end_exclusive must be above RAM base");
    PHYS_RAM_END_EXCL.store(ram_end_exclusive, Ordering::Release);

    // 初始化帧分配器
    let kernel_end_addr : usize;
    unsafe {
        core::arch::asm!("la {}, kernel_end", out(reg) kernel_end_addr);
    }
    let start_ppn = (kernel_end_addr + PAGE_SIZE - 1) / PAGE_SIZE;
    let end_ppn = ram_end_exclusive / PAGE_SIZE;
    frame_alloctor::init_frame_allocator(PhysPageNum(start_ppn),
                                         PhysPageNum(end_ppn));

    let mut aspace = LoongArch64AddressSpace::new_kernel()
        .expect("kernel_mm: LoongArch64AddressSpace::new_kernel failed");

    let map_identity = |aspace : &mut LoongArch64AddressSpace,
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
                 LOONGARCH64_RAM_BASE,
                 ram_end_exclusive,
                 PagePerm::R | PagePerm::W | PagePerm::X,
                 "RAM");

    // 访问 UART、PLIC/MSI、PCI ECAM 等低地址 MMIO 必须映射；与 `-m` 无关。
    map_identity(&mut aspace,
                 LOONGARCH64_LOW_MMIO_START,
                 LOONGARCH64_LOW_MMIO_END,
                 PagePerm::R | PagePerm::W,
                 "low MMIO");

    // VirtIO PCI transport 会在该窗口内分配 BAR，启用 PGDL 后也要恒等映射。
    map_identity(&mut aspace,
                 LOONGARCH64_PCI_MMIO_START,
                 LOONGARCH64_PCI_MMIO_END,
                 PagePerm::R | PagePerm::W,
                 "PCI MMIO");

    // 选一枚帧池内真实 RAM 帧，用已建立的 RAM 恒等映射做 PGDL 切换后的访存探针。
    // 避免额外低地址 VA 受 LoongArch64 PGDL/PGDH 选择规则影响。
    let probe_ppn = frame_alloc_result().expect("kernel_mm: frame oom for probe");
    let probe_va = VirtAddr(probe_ppn.0 * PAGE_SIZE + 0x2A0);

    let pgdl_target = aspace.satp_value();
    runtime::logging::trace!("[kernel-mm] identity map RAM [{:#x},{:#x}) MMIO [{:#x},{:#x}) pgdl \
                              target={:#x}",
                             LOONGARCH64_RAM_BASE,
                             ram_end_exclusive,
                             LOONGARCH64_LOW_MMIO_START,
                             LOONGARCH64_LOW_MMIO_END,
                             pgdl_target);
    platform::arch::paging::activate_address_space_token_and_flush(pgdl_target);
    platform::arch::paging::enable_paging();
    assert_eq!(platform::arch::paging::active_address_space_token(),
               pgdl_target,
               "kernel_mm: pgdl mismatch");

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
    api_v0::kernel_satp::set(pgdl_target);
    api_v0::user_aspace_lifecycle::register_drop_user_aspace_hook(crate::kernel_mm_impl::drop_user_aspace);
    api_v0::user_aspace_lifecycle::register_aspace_cpu_hooks(crate::user_aspace::mark_active,
                                                              crate::user_aspace::mark_inactive);
    api_v0::user_mapping::register_snapshot_user_mappings_hook(
        crate::user_aspace::snapshot_user_mappings,
    );
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
            Ok(()) => {}
            Err(MmError::AlreadyMapped) => {}
            Err(e) => panic!("kernel_mm: map {:?} -> {:?}: {:?}",
                             vpn, ppn, e),
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
