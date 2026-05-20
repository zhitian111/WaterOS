//! 内核未实现的 syscall 路径：开发期直接 panic，避免用户态把错误码当指针继续执行。

extern crate alloc;

/// 报告不支持的 syscall 语义并终止内核（不返回用户态）。
#[inline(never)]
pub(crate) fn syscall_unsupported(detail : &str) -> ! {
    panic!("[syscall] unsupported: {detail}");
}

/// 未知 syscall 号。
#[inline(never)]
pub(crate) fn syscall_unknown(nr : usize, args : abi::syscall_args::SyscallArgs) -> ! {
    let r = args.as_regs();
    syscall_unsupported(&alloc::format!(
        "unknown nr={nr} args=[{:#x},{:#x},{:#x},{:#x},{:#x},{:#x}]",
        r[0], r[1], r[2], r[3], r[4], r[5]
    ));
}
