//! `sendfile(2)`：在已打开 fd 间拷贝数据（内核缓冲，最小语义）。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::vec::Vec;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};
use crate::vfs_util::{vfs_error_to_errno, vfs_io_at_error_to_errno};

const IO_CHUNK: usize = 64 * 1024;

// 本方法代码由AI完成
pub(crate) fn sys_sendfile(args: SyscallArgs) -> UserRet {
    let out_fd = args.arg(0);
    let in_fd = args.arg(1);
    let offset_ptr = args.arg(2);
    let count = args.arg(3);

    if in_fd == out_fd {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if count == 0 {
        return UserRet::from_success(0);
    }

    let use_read_at = offset_ptr != 0;
    let mut file_offset = if use_read_at {
        match copy_from_user_struct::<u64>(offset_ptr) {
            Ok(v) => v,
            Err(e) => return UserRet::from_error(e),
        }
    } else {
        0
    };

    let mut transferred = 0usize;
    let mut buf = Vec::new();
    buf.resize(IO_CHUNK.min(count), 0);

    while transferred < count {
        let remaining = count - transferred;
        let chunk = remaining.min(IO_CHUNK);
        if buf.len() != chunk {
            buf.resize(chunk, 0);
        }

        let n = if use_read_at {
            match vfs::fd::with_current_io(in_fd, |handle| handle.read_at(file_offset, &mut buf)) {
                Ok(n) => n,
                Err(err) => return UserRet::from_error(vfs_io_at_error_to_errno(err)),
            }
        } else {
            match vfs::fd::with_current_io(in_fd, |handle| handle.read(&mut buf)) {
                Ok(n) => n,
                Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
            }
        };

        if n == 0 {
            break;
        }

        let mut written = 0usize;
        while written < n {
            match vfs::fd::with_current_io(out_fd, |handle| handle.write(&buf[written..n])) {
                Ok(0) => break,
                Ok(w) => {
                    written += w;
                    transferred += w;
                    if use_read_at {
                        file_offset = match file_offset.checked_add(w as u64) {
                            Some(v) => v,
                            None => return UserRet::from_error(ErrNo::EINVAL),
                        };
                    }
                }
                Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
            }
        }
        if written < n {
            break;
        }
    }

    if use_read_at {
        if copy_to_user_struct(offset_ptr, &file_offset).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }

    UserRet::from_success(transferred)
}
