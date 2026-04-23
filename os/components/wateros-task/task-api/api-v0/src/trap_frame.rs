/// 任务自己持有的最近一次 trap 上下文快照。
///
/// 布局刻意与当前 RISC-V `TrapContext` 保持一致，方便 trap 路径直接
/// 整体复制，而不需要逐字段转换。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskTrapFrame {
    pub x: [usize; 32],
    pub sstatus: usize,
    pub sepc: usize,
    pub scause: usize,
    pub stval: usize,
}

const RISCV_SSTATUS_SIE: usize = 1 << 1;
const RISCV_SSTATUS_SPIE: usize = 1 << 5;
const RISCV_SSTATUS_SPP: usize = 1 << 8;

impl TaskTrapFrame {
    /// 返回原始 trap 原因编码。
    #[inline]
    pub const fn raw_cause(&self) -> usize { self.scause }

    /// 返回发生 trap 时保存的程序计数器。
    #[inline]
    pub const fn user_pc(&self) -> usize { self.sepc }

    /// 返回发生 trap 时保存的用户栈指针。
    #[inline]
    pub const fn user_sp(&self) -> usize { self.x[2] }

    /// 返回与 trap 关联的故障地址或附加值。
    #[inline]
    pub const fn fault_addr(&self) -> usize { self.stval }

    /// 判断当前 trap frame 在恢复时是否会返回到用户态。
    #[inline]
    pub const fn returns_to_user(&self) -> bool { (self.sstatus & RISCV_SSTATUS_SPP) == 0 }

    /// 判断当前 trap frame 在恢复时是否会返回到内核态。
    #[inline]
    pub const fn returns_to_kernel(&self) -> bool { !self.returns_to_user() }

    /// 设置恢复后的用户 PC。
    #[inline]
    pub fn set_user_pc(&mut self, pc: usize) { self.sepc = pc; }

    /// 在当前用户 PC 基础上前进指定字节数。
    #[inline]
    pub fn add_user_pc(&mut self, bytes: usize) {
        self.sepc = self.sepc.wrapping_add(bytes);
    }

    /// 设置恢复后的用户栈指针。
    #[inline]
    pub fn set_user_sp(&mut self, sp: usize) { self.x[2] = sp; }

    /// 设置 syscall 返回值寄存器。
    #[inline]
    pub fn set_syscall_ret(&mut self, ret: isize) { self.x[10] = ret as usize; }

    /// 将该 trap frame 标记为恢复到用户态。
    #[inline]
    pub fn set_return_to_user(&mut self) {
        self.sstatus &= !RISCV_SSTATUS_SPP;
        self.sstatus &= !RISCV_SSTATUS_SIE;
        self.sstatus |= RISCV_SSTATUS_SPIE;
    }

    /// 将该 trap frame 标记为恢复到内核态。
    #[inline]
    pub fn set_return_to_kernel(&mut self) { self.sstatus |= RISCV_SSTATUS_SPP; }

    /// 准备一次最小的用户态返回现场。
    #[inline]
    pub fn prepare_user_return(&mut self, entry_pc: usize, user_sp: usize) {
        self.set_user_pc(entry_pc);
        self.set_user_sp(user_sp);
        self.set_return_to_user();
    }
}
