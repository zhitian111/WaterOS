//! `lseek(2)`：调整已打开文件的读写偏移。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsSeekWhence;

use crate::vfs_util::vfs_error_to_errno;

const SEEK_SET : usize = 0;
const SEEK_CUR : usize = 1;
const SEEK_END : usize = 2;

// 本方法代码由AI完成
pub(crate) fn sys_lseek(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let offset = args.arg(1) as i64;
    let whence = args.arg(2);

    let whence = match whence {
        SEEK_SET => VfsSeekWhence::Set,
        SEEK_CUR => VfsSeekWhence::Cur,
        SEEK_END => VfsSeekWhence::End,
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };

    match vfs::fd::with_current_io(fd, |handle| handle.seek(offset, whence)) {
        Ok(pos) => UserRet::from_success(pos as usize),
        Err(e) => {
            let errno = match vfs_error_to_errno(e) {
                ErrNo::EINVAL => ErrNo::ESPIPE,
                other => other,
            };
            UserRet::from_error(errno)
        }
    }
}
