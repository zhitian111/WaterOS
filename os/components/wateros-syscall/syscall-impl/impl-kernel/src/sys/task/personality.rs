//! `personality(2)`：查询或选择当前内核支持的执行域。

use api_v0::{ErrNo, SyscallArgs, UserRet};

const PERSONALITY_QUERY : u32 = u32::MAX;
const PER_LINUX : u32 = 0;
/// `UNAME26`（0x20000）：uname() 报告 3.2.0 版，只改变显示不改变执行域。
const UNAME26 : u32 = 0x0020000;

/// WaterOS 目前只提供本架构原生的 Linux 执行域，也没有 32 位兼容层或 ASLR
/// personality 标志。查询、设置 `PER_LINUX` 以及 `UNAME26`（仅影响 uname
/// 显示，LTP uname04 会设置它）是有意义且完整的；其它值必须返回 EINVAL，
/// 不能静默接受一个不会生效的执行域。
pub(crate) fn sys_personality(args : SyscallArgs) -> UserRet {
    match args.arg(0) as u32 {
        PERSONALITY_QUERY | PER_LINUX | UNAME26 => UserRet::from_success(PER_LINUX as usize),
        _ => UserRet::from_error(ErrNo::EINVAL),
    }
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    assert_eq!(PERSONALITY_QUERY, u32::MAX);
    assert_eq!(PER_LINUX, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_native_personality_is_supported() {
        assert!(matches!(PERSONALITY_QUERY, u32::MAX));
        assert_eq!(PER_LINUX, 0);
    }
}
