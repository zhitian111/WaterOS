use core::arch::asm;

/// RISC-V Sv39 分页控制原语。
pub struct Riscv64Paging;

impl Riscv64Paging {
    /// 读取 satp 当前值，用于分页启用前后的观测。
    #[inline]
    pub fn read_satp() -> usize {
        let value: usize;
        unsafe {
            asm!("csrr {0}, satp", out(reg) value);
        }
        value
    }

    /// 写入 satp 后立刻执行全局 sfence.vma，确保页表切换后翻译可见。
    #[inline]
    pub fn write_satp_and_flush(satp: usize) {
        unsafe {
            asm!("csrw satp, {0}", in(reg) satp);
            asm!("sfence.vma x0, x0");
        }
    }
}
