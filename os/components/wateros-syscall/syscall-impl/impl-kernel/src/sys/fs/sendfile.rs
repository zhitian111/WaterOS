//! `sendfile(2)`：在两个已打开 fd 之间搬运数据。
//!
//! 顺序输入使用 VFS read lease，输出短写时只消费已交付部分；显式 offset
//! 使用 `read_at`，同样只按实际写入量推进用户指针。

extern crate alloc;

use api_v0::{ErrNo, SyscallArgs, UserRet};
use vfs::api::{VfsCopyProgress, VfsError, VfsReadFinish};

use crate::fallible_buf::try_kbuf;
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};
use crate::vfs_util::{vfs_error_to_errno, vfs_io_at_error_to_errno};

const MAX_RW_COUNT : usize = 0x7fff_f000;
const IO_CHUNK : usize = 64 * 1024;

fn validate_fds(input_fd : usize, output_fd : usize) -> Result<(), ErrNo> {
    if input_fd == output_fd {
        return Err(ErrNo::EINVAL);
    }
    vfs::fd::with_current_io(input_fd, |handle| handle.validate_read_access())
        .map_err(vfs_error_to_errno)?;
    vfs::fd::with_current_io(output_fd, |handle| {
        const O_ACCMODE : u32 = 3;
        const O_RDONLY : u32 = 0;
        if handle.open_accmode() & O_ACCMODE == O_RDONLY {
            Err(VfsError::BadFd)
        } else {
            Ok(())
        }
    }).map_err(vfs_error_to_errno)
}

fn write_output(output_fd : usize, bytes : &[u8]) -> Result<usize, (usize, ErrNo)> {
    let mut written = 0usize;
    while written < bytes.len() {
        match vfs::fd::with_current_io_detached(output_fd, |handle| {
                  handle.write(&bytes[written..])
              }) {
            Ok(0) => break,
            Ok(count) => written += count,
            Err(error) => return Err((written, vfs_error_to_errno(error))),
        }
    }
    Ok(written)
}

fn finish(transferred : usize, terminal_error : Option<ErrNo>) -> UserRet {
    if transferred > 0 || terminal_error.is_none() {
        UserRet::from_success(transferred)
    } else {
        let error = terminal_error.unwrap();
        if error == ErrNo::EPIPE {
            let _ = crate::sys::ipc::signal::raise_current_thread(ipc::signal::SIGPIPE);
        }
        UserRet::from_error(error)
    }
}

pub(crate) fn sys_sendfile(args : SyscallArgs) -> UserRet {
    let output_fd = args.arg(0);
    let input_fd = args.arg(1);
    let offset_ptr = args.arg(2);
    let requested = args.arg(3).min(MAX_RW_COUNT);

    if let Err(error) = validate_fds(input_fd, output_fd) {
        return UserRet::from_error(error);
    }
    if requested == 0 {
        return UserRet::from_success(0);
    }

    if offset_ptr == 0 {
        return sendfile_sequential(input_fd, output_fd, requested);
    }
    let offset = match copy_from_user_struct::<i64>(offset_ptr) {
        Ok(offset) if offset >= 0 => offset as u64,
        Ok(_) => return UserRet::from_error(ErrNo::EINVAL),
        Err(error) => return UserRet::from_error(error),
    };
    sendfile_positional(input_fd, output_fd, offset_ptr, offset, requested)
}

fn sendfile_sequential(input_fd : usize, output_fd : usize, requested : usize) -> UserRet {
    let mut transferred = 0usize;
    let mut terminal_error = None;
    while transferred < requested {
        let chunk = (requested - transferred).min(IO_CHUNK);
        let lease = loop {
            let prepared = match vfs::fd::prepare_current_read(input_fd, chunk) {
                Ok(prepared) => prepared,
                Err(error) => {
                    terminal_error = Some(vfs_error_to_errno(error));
                    return finish(transferred, terminal_error);
                }
            };
            match prepared.acquire() {
                Ok(lease) => break lease,
                Err(VfsError::Busy) => task::yield_now(),
                Err(error) => {
                    terminal_error = Some(vfs_error_to_errno(error));
                    return finish(transferred, terminal_error);
                }
            }
        };
        if lease.bytes().is_empty() {
            let _ = lease.finish(VfsCopyProgress { copied : 0,
                                                   complete : true });
            break;
        }
        let (written, write_error) = match write_output(output_fd, lease.bytes()) {
            Ok(written) => (written, None),
            Err((written, error)) => (written, Some(error)),
        };
        match lease.finish(VfsCopyProgress { copied : written,
                                             complete : write_error.is_none() }) {
            Ok(VfsReadFinish::Bytes(_)) => {}
            Ok(VfsReadFinish::Fault) => {
                terminal_error = Some(ErrNo::EFAULT);
                break;
            }
            Err(error) => {
                terminal_error = Some(vfs_error_to_errno(error));
                break;
            }
        }
        transferred += written;
        if write_error.is_some() || written == 0 {
            terminal_error = write_error;
            break;
        }
    }
    finish(transferred, terminal_error)
}

fn sendfile_positional(input_fd : usize,
                       output_fd : usize,
                       offset_ptr : usize,
                       mut offset : u64,
                       requested : usize)
                       -> UserRet {
    let mut buffer = match try_kbuf(requested.min(IO_CHUNK), IO_CHUNK) {
        Ok(buffer) => buffer,
        Err(error) => return UserRet::from_error(error),
    };
    let mut transferred = 0usize;
    let mut terminal_error = None;
    while transferred < requested {
        let chunk = (requested - transferred).min(buffer.len());
        let read = match vfs::fd::with_current_io_detached(input_fd, |handle| {
                  handle.read_at(offset, &mut buffer[..chunk])
              }) {
            Ok(read) => read,
            Err(error) => {
                terminal_error = Some(vfs_io_at_error_to_errno(error));
                break;
            }
        };
        if read == 0 {
            break;
        }
        let (written, write_error) = match write_output(output_fd, &buffer[..read]) {
            Ok(written) => (written, None),
            Err((written, error)) => (written, Some(error)),
        };
        offset = match offset.checked_add(written as u64) {
            Some(offset) => offset,
            None => {
                terminal_error = Some(ErrNo::EFBIG);
                break;
            }
        };
        transferred += written;
        if write_error.is_some() || written < read {
            terminal_error = write_error;
            break;
        }
    }
    let offset = match i64::try_from(offset) {
        Ok(offset) => offset,
        Err(_) => return UserRet::from_error(ErrNo::EFBIG),
    };
    if let Err(error) = copy_to_user_struct(offset_ptr, &offset) {
        return UserRet::from_error(error);
    }
    finish(transferred, terminal_error)
}
