//! RISC-V 指令缓存同步系统调用。

use api_v0::{ErrNo, SyscallArgs, UserRet};
use platform::smp::PlatformSmpError;

/// 将同步限制在调用线程当前 CPU。
const SYS_RISCV_FLUSH_ICACHE_LOCAL: usize = 1;

pub(crate) fn sys_riscv_flush_icache(args: SyscallArgs) -> UserRet {
    let start = args.arg(0);
    let end = args.arg(1);
    let flags = args.arg(2);

    match flush_icache(start, end, flags) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

fn flush_icache(_start: usize, _end: usize, flags: usize) -> Result<(), ErrNo> {
    if flags & !SYS_RISCV_FLUSH_ICACHE_LOCAL != 0 {
        return Err(ErrNo::EINVAL);
    }

    // Linux 当前把 start/end 视为提示，并刷新整个本地指令缓存；`fence.i` 同时保证
    // 当前 hart 之前的存储先于之后的取指可见。
    unsafe { core::arch::asm!("fence.i", options(nostack, preserves_flags)) };

    if flags & SYS_RISCV_FLUSH_ICACHE_LOCAL != 0 {
        return Ok(());
    }

    let current = platform::arch::cpu::current_cpu_id();
    let mut remote = task::online_cpu_mask();
    remote.remove(current);
    if remote.is_empty() {
        return Ok(());
    }

    platform::smp::flush_icache_remote(remote).map_err(|error| match error {
        PlatformSmpError::Unsupported => ErrNo::ENOSYS,
        PlatformSmpError::InvalidCpu => ErrNo::EINVAL,
        PlatformSmpError::AlreadyAvailable | PlatformSmpError::Firmware(_) => ErrNo::EIO,
    })
}
