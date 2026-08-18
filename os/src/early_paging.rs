//! 2K1000 早期静态内核页表。
//!
//! U-Boot 跳转时 `CRMD.PG=1/DA=0`，但现有 WaterOS 的正式 `kernel_mm::init`
//! 在 heap/frame allocator 之后才运行。为了让高 cached 数据区的原子操作、
//! console 锁在进入 Rust main 早期就能用，这里用静态 BSS 页表直接映射
//! 2K1000 bank1 高 cached RAM。

#![allow(dead_code)]

const PAGE_SHIFT: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_SHIFT;
const PTE_V: u64 = 1 << 0;
const PTE_D: u64 = 1 << 1;
const PTE_MAT_CACHED: u64 = 1 << 4;
const PTE_P: u64 = 1 << 7;
const PTE_W: u64 = 1 << 8;
const PTE_NX: u64 = 1 << 62;
const RAM_START_VA: usize = 0x9000_0000_9000_0000;
const RAM_END_VA: usize = 0x9000_0000_c000_0000;
const RAM_LEAF_COUNT: usize = 384;
const PHYS_MASK: usize = 0x0000_ffff_ffff_ffff;

#[repr(C, align(4096))]
struct PageTable([u64; 512]);

#[repr(C, align(4096))]
struct LeafTables([[u64; 512]; RAM_LEAF_COUNT]);

static mut ROOT: PageTable = PageTable([0; 512]);
static mut MIDDLE: PageTable = PageTable([0; 512]);
static mut LEAVES: LeafTables = LeafTables([[0; 512]; RAM_LEAF_COUNT]);

#[inline]
fn phys_addr(ptr: *const u8) -> usize {
    ptr as usize & PHYS_MASK
}

#[inline]
fn table_entry(ppn: usize) -> u64 {
    (ppn as u64) << PAGE_SHIFT
}

#[inline]
fn leaf_entry(ppn: usize, flags: u64) -> u64 {
    (ppn as u64) << PAGE_SHIFT | flags
}

/// 静态页表映射 `RAM_START_VA..RAM_END_VA`，并安装到 PGDL。
///
/// 仅使用普通 store 与 CSR 操作，不依赖 heap/frame allocator/spin lock。
pub fn init() {
    let root_ppn = phys_addr(unsafe { core::ptr::addr_of!(ROOT) as *const u8 }) / PAGE_SIZE;
    let middle_ppn = phys_addr(unsafe { core::ptr::addr_of!(MIDDLE) as *const u8 }) / PAGE_SIZE;
    let leaves_ppn =
        phys_addr(unsafe { core::ptr::addr_of!(LEAVES) as *const u8 }) / PAGE_SIZE;

    let start_vpn = (RAM_START_VA >> PAGE_SHIFT) & ((1 << 27) - 1);
    let end_vpn = (RAM_END_VA >> PAGE_SHIFT) & ((1 << 27) - 1);
    let flags_rwx = PTE_V | PTE_D | PTE_MAT_CACHED | PTE_P | PTE_W;

    unsafe {
        let root = &mut *core::ptr::addr_of_mut!(ROOT);
        let middle = &mut *core::ptr::addr_of_mut!(MIDDLE);
        let leaves = &mut *core::ptr::addr_of_mut!(LEAVES);
        let idx2 = (start_vpn >> 18) & 0x1ff;
        root.0[idx2] = table_entry(middle_ppn);

        let first_idx1 = (start_vpn >> 9) & 0x1ff;
        for leaf_index in 0..RAM_LEAF_COUNT {
            let idx1 = first_idx1 + leaf_index;
            middle.0[idx1] = table_entry(leaves_ppn + leaf_index * PAGE_SIZE / PAGE_SIZE);
        }

        for vpn in start_vpn..end_vpn {
            let idx0 = (vpn >> 0) & 0x1ff;
            let idx1 = (vpn >> 9) & 0x1ff;
            let leaf_index = idx1 - first_idx1;
            leaves.0[leaf_index][idx0] = leaf_entry(vpn, flags_rwx);
        }
    }

    platform::arch::paging::activate_address_space_token_and_flush(root_ppn);
    platform::arch::paging::enable_paging();
}
