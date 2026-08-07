//! OpenSBI 传入的启动参数及其类型化视图。

use api_v0::boot::PlatformBootArgs;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

const COMMAND_LINE_CAPACITY: usize = 1024;

struct CommandLineCell(UnsafeCell<[u8; COMMAND_LINE_CAPACITY]>);

unsafe impl Sync for CommandLineCell {}

static COMMAND_LINE: CommandLineCell = CommandLineCell(UnsafeCell::new([0; COMMAND_LINE_CAPACITY]));
static COMMAND_LINE_LEN: AtomicUsize = AtomicUsize::new(0);

/// Copy `/chosen/bootargs` out of the firmware DTB while it is still directly
/// addressable. Only the BSP calls this function.
pub unsafe fn init_command_line(_arg0: usize, dtb_pa: usize, _arg2: usize) {
    if dtb_pa == 0 || COMMAND_LINE_LEN.load(Ordering::Acquire) != 0 {
        return;
    }
    let Ok(fdt) = (unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) }) else {
        return;
    };
    let Some(chosen) = fdt.find_node("/chosen") else {
        return;
    };
    let Some(property) = chosen.property("bootargs") else {
        return;
    };
    let bytes = property.value.split(|byte| *byte == 0).next().unwrap_or(&[]);
    let len = bytes.len().min(COMMAND_LINE_CAPACITY - 1);
    unsafe {
        let storage = &mut *COMMAND_LINE.0.get();
        storage[..len].copy_from_slice(&bytes[..len]);
        storage[len] = 0;
    }
    if len != 0 {
        COMMAND_LINE_LEN.store(len, Ordering::Release);
    }
}

pub fn command_line() -> Option<&'static str> {
    let len = COMMAND_LINE_LEN.load(Ordering::Acquire);
    if len == 0 {
        return None;
    }
    let bytes = unsafe { &(&*COMMAND_LINE.0.get())[..len] };
    core::str::from_utf8(bytes).ok()
}

#[derive(Debug, Clone, Copy)]
/// OpenSBI 交给内核入口的原始 `a0/a1` 参数。
///
/// BOOT_CONTRACT: QEMU RISC-V `virt` 中分别约定为 hart id 与 DTB 物理地址；在
/// platform/MM 明确建立映射前，不能把 `arg1` 解释为可直接解引用的虚拟地址。
pub struct QEMURiscv64OpenSBIBootArgs {
    /// OpenSBI `a0`，当前映射为逻辑 CPU/hart id。
    arg0: usize,
    /// OpenSBI `a1`，当前映射为 DTB 物理地址。
    arg1: usize,
}

impl PlatformBootArgs for QEMURiscv64OpenSBIBootArgs {
    #[inline]
    fn arg0(&self) -> Option<usize> {
        Some(self.arg0)
    }
    #[inline]
    fn arg1(&self) -> Option<usize> {
        Some(self.arg1)
    }
}

impl QEMURiscv64OpenSBIBootArgs {
    /// 从 arch 启动入口保存的寄存器值构造参数包；不验证 DTB 内容。
    #[inline]
    pub fn new(arg0: usize, arg1: usize) -> Self {
        Self { arg0, arg1 }
    }
}

pub use QEMURiscv64OpenSBIBootArgs as BootArgs;
