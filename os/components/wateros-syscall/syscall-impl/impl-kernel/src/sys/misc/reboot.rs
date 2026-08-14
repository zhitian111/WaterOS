//! `reboot(2)`：校验 Linux magic/权限后转交平台 reset 后端。

use api_v0::{ErrNo, SyscallArgs, UserRet};
use platform::reset::{PlatformResetError, PlatformResetReason};

const LINUX_REBOOT_MAGIC1 : u32 = 0xFEE1_DEAD;
const LINUX_REBOOT_MAGIC2 : u32 = 672_274_793;
const LINUX_REBOOT_MAGIC2A : u32 = 85_072_278;
const LINUX_REBOOT_MAGIC2B : u32 = 369_367_448;
const LINUX_REBOOT_MAGIC2C : u32 = 537_993_216;

const LINUX_REBOOT_CMD_RESTART : u32 = 0x0123_4567;
const LINUX_REBOOT_CMD_HALT : u32 = 0xCDEF_0123;
const LINUX_REBOOT_CMD_POWER_OFF : u32 = 0x4321_FEDC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RebootAction {
    Restart,
    Shutdown,
}

fn decode_reboot_action(magic1 : u32, magic2 : u32, command : u32) -> Result<RebootAction, ErrNo> {
    if magic1 != LINUX_REBOOT_MAGIC1 ||
       !matches!(magic2,
                 LINUX_REBOOT_MAGIC2 |
                 LINUX_REBOOT_MAGIC2A |
                 LINUX_REBOOT_MAGIC2B |
                 LINUX_REBOOT_MAGIC2C)
    {
        return Err(ErrNo::EINVAL);
    }

    match command {
        LINUX_REBOOT_CMD_RESTART => Ok(RebootAction::Restart),
        // QEMU 两个平台都没有“停机但不掉电”的独立出口；HALT 降级为关机，
        // 保证 BusyBox halt 不会让评测机永久停在一个不可恢复的 CPU 循环里。
        LINUX_REBOOT_CMD_HALT | LINUX_REBOOT_CMD_POWER_OFF => Ok(RebootAction::Shutdown),
        _ => Err(ErrNo::EINVAL),
    }
}

fn reset_error_to_errno(error : PlatformResetError) -> ErrNo {
    match error {
        PlatformResetError::Unsupported => ErrNo::EOPNOTSUPP,
        PlatformResetError::Unavailable => ErrNo::EAGAIN,
        PlatformResetError::Failed => ErrNo::EIO,
    }
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    assert_eq!(decode_reboot_action(LINUX_REBOOT_MAGIC1,
                                    LINUX_REBOOT_MAGIC2,
                                    LINUX_REBOOT_CMD_RESTART),
               Ok(RebootAction::Restart));
    assert_eq!(decode_reboot_action(0,
                                    LINUX_REBOOT_MAGIC2,
                                    LINUX_REBOOT_CMD_RESTART),
               Err(ErrNo::EINVAL));
}

pub(crate) fn sys_reboot(args : SyscallArgs) -> UserRet {
    if cred::current_credentials().effective_uid
                                  .0 !=
       0
    {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let action = match decode_reboot_action(args.arg(0) as u32,
                                            args.arg(1) as u32,
                                            args.arg(2) as u32)
    {
        Ok(action) => action,
        Err(error) => return UserRet::from_error(error),
    };

    let result = match action {
        RebootAction::Restart => platform::reset::reboot(PlatformResetReason::NoReason),
        RebootAction::Shutdown => platform::reset::shutdown(PlatformResetReason::NoReason),
    };
    match result {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(reset_error_to_errno(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_linux_magic_variants() {
        for magic2 in [LINUX_REBOOT_MAGIC2,
                       LINUX_REBOOT_MAGIC2A,
                       LINUX_REBOOT_MAGIC2B,
                       LINUX_REBOOT_MAGIC2C]
        {
            assert_eq!(decode_reboot_action(LINUX_REBOOT_MAGIC1,
                                            magic2,
                                            LINUX_REBOOT_CMD_RESTART),
                       Ok(RebootAction::Restart));
        }
    }

    #[test]
    fn rejects_bad_magic_and_unknown_commands() {
        assert_eq!(decode_reboot_action(0,
                                        LINUX_REBOOT_MAGIC2,
                                        LINUX_REBOOT_CMD_RESTART),
                   Err(ErrNo::EINVAL));
        assert_eq!(decode_reboot_action(LINUX_REBOOT_MAGIC1,
                                        LINUX_REBOOT_MAGIC2,
                                        1),
                   Err(ErrNo::EINVAL));
    }
}
