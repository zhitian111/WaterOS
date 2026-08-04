//! LoongArch QEMU `virt` 固件传入的原始参数。

use api_v0::boot::PlatformBootArgs;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

const COMMAND_LINE_CAPACITY: usize = 1024;
const MAX_FIRMWARE_ARGS: usize = 64;
/// QEMU direct ELF boot currently clears argc/argv. The run scripts mirror the
/// command line into this early-RAM mailbox with `-device loader`.
// Keep the command-line mailbox above the linked kernel image/heap while
// leaving the top of QEMU RAM available for firmware-created data such as FDT.
const QEMU_COMMAND_LINE_MAILBOX: usize = 0xA000_0000;
const QEMU_COMMAND_LINE_MAGIC: &[u8; 7] = b"WOSCMD1";

struct CommandLineCell(UnsafeCell<[u8; COMMAND_LINE_CAPACITY]>);

unsafe impl Sync for CommandLineCell {}

static COMMAND_LINE: CommandLineCell = CommandLineCell(UnsafeCell::new([0; COMMAND_LINE_CAPACITY]));
static COMMAND_LINE_LEN: AtomicUsize = AtomicUsize::new(0);

/// Preserve QEMU direct-boot argv before the generic arch entry repurposes the
/// argument registers. QEMU places the text supplied through `-append` in the
/// firmware argument vector.
pub unsafe fn init_command_line(argc: usize, argv: usize, _envp: usize) {
    if COMMAND_LINE_LEN.load(Ordering::Acquire) != 0 {
        return;
    }
    let storage = unsafe { &mut *COMMAND_LINE.0.get() };
    let mut written = 0usize;
    if argc > 1 && argc <= MAX_FIRMWARE_ARGS && argv != 0 {
        for index in 1..argc {
            let pointer = unsafe { *((argv as *const usize).add(index)) };
            if pointer == 0 {
                break;
            }
            if written != 0 && written < COMMAND_LINE_CAPACITY - 1 {
                storage[written] = b' ';
                written += 1;
            }
            let mut offset = 0usize;
            while written < COMMAND_LINE_CAPACITY - 1 {
                let byte = unsafe { *((pointer as *const u8).add(offset)) };
                if byte == 0 {
                    break;
                }
                storage[written] = byte;
                written += 1;
                offset += 1;
            }
        }
    }

    if written == 0 {
        let mailbox = QEMU_COMMAND_LINE_MAILBOX as *const u8;
        let valid = QEMU_COMMAND_LINE_MAGIC.iter().enumerate().all(|(index, expected)| {
            unsafe { core::ptr::read_volatile(mailbox.add(index)) == *expected }
        });
        if valid {
            while written < COMMAND_LINE_CAPACITY - 1 {
                let byte = unsafe {
                    core::ptr::read_volatile(mailbox.add(QEMU_COMMAND_LINE_MAGIC.len() + written))
                };
                if byte == 0 {
                    break;
                }
                storage[written] = byte;
                written += 1;
            }
        }
    }
    storage[written] = 0;
    if written != 0 {
        COMMAND_LINE_LEN.store(written, Ordering::Release);
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

/// QEMU LoongArch `virt` 在 direct-kernel boot 时放置自动生成 DTB 的物理地址。
/// 该地址属于 machine profile，不是 LoongArch ISA 启动 ABI。
pub const DEVICE_TREE_PHYS_ADDR : usize = 0x0010_0000;

#[inline]
pub const fn device_tree_phys_addr() -> usize { DEVICE_TREE_PHYS_ADDR }

#[derive(Debug, Clone, Copy)]
/// LoongArch QEMU 入口透传的三项原始参数。
///
/// BOOT_CONTRACT: 该 profile 当前不把这三项绑定为固定 ABI 语义；使用者应在真正
/// 需要某项前先由 machine/firmware 文档确认，避免照搬 RISC-V 的 hart/DTB 约定。
pub struct QEMULoongArch64VirtBootArgs {
    arg0 : usize,
    arg1 : usize,
    arg2 : usize,
}

impl QEMULoongArch64VirtBootArgs {
    /// 从 arch 启动入口保存的参数构造，不做地址或 CPU 编号校验。
    #[inline]
    pub const fn new(arg0 : usize, arg1 : usize, arg2 : usize) -> Self { Self { arg0, arg1, arg2 } }
}

impl PlatformBootArgs for QEMULoongArch64VirtBootArgs {
    #[inline]
    fn arg0(&self) -> Option<usize> { Some(self.arg0) }
    #[inline]
    fn arg1(&self) -> Option<usize> { Some(self.arg1) }
    #[inline]
    fn arg2(&self) -> Option<usize> { Some(self.arg2) }
}

pub use QEMULoongArch64VirtBootArgs as BootArgs;
