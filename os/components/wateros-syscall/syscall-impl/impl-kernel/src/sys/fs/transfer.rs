//! 内核内文件搬运 syscall：`copy_file_range(2)`、`splice(2)`、`tee(2)` 与
//! `vmsplice(2)`。
//!
//! 两条路径都按“实际写入多少，输入位置才前进多少”的规则提交进度。
//! `splice` 从 pipe 读取时使用 VFS read lease，输出短写或失败不会吞掉尚未
//! 交付的数据。这一点是不能用普通 `read` + `write` 临时拼接的核心原因。

extern crate alloc;

use alloc::vec::Vec;

use api_v0::{ErrNo, SyscallArgs, UserRet};
use vfs::api::{
    VfsCopyProgress, VfsError, VfsNodeType, VfsReadFinish, VfsReadLease, VfsSeekWhence,
};

use crate::fallible_buf::try_kbuf;
use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user_struct};
use crate::vfs_util::{vfs_error_to_errno, vfs_io_at_error_to_errno};

const MAX_RW_COUNT : usize = 0x7FFF_F000;
const IO_CHUNK : usize = 64 * 1024;
const IOV_MAX : usize = 1024;
const O_APPEND : u32 = 0o2000;

const SPLICE_F_MOVE : usize = 0x01;
const SPLICE_F_NONBLOCK : usize = 0x02;
const SPLICE_F_MORE : usize = 0x04;
const SPLICE_F_GIFT : usize = 0x08;
const SPLICE_FLAGS : usize = SPLICE_F_MOVE | SPLICE_F_NONBLOCK | SPLICE_F_MORE | SPLICE_F_GIFT;

#[derive(Clone, Copy)]
struct FileCursor {
    /// 用户态显式偏移指针；为 0 时使用打开文件描述的当前偏移。
    user_ptr : usize,
    /// 当前文件偏移，单位为字节。
    position : u64,
}

impl FileCursor {
    fn import(fd : usize, user_ptr : usize) -> Result<Self, ErrNo> {
        let position = if user_ptr == 0 {
            vfs::fd::with_current_io(fd, |handle| {
                handle.seek(0, VfsSeekWhence::Cur)
            }).map_err(seek_errno)?
        } else {
            let offset = copy_from_user_struct::<i64>(user_ptr)?;
            u64::try_from(offset).map_err(|_| ErrNo::EINVAL)?
        };
        Ok(Self { user_ptr, position })
    }

    fn advance(&mut self, bytes : usize) -> Result<(), ErrNo> {
        self.position = self.position
                            .checked_add(bytes as u64)
                            .ok_or(ErrNo::EFBIG)?;
        Ok(())
    }

    fn publish(self, fd : usize) -> Result<(), ErrNo> {
        if self.user_ptr == 0 {
            let offset = i64::try_from(self.position).map_err(|_| ErrNo::EFBIG)?;
            vfs::fd::with_current_io(fd, |handle| {
                handle.seek(offset, VfsSeekWhence::Set)
                      .map(|_| ())
            }).map_err(seek_errno)
        } else {
            let offset = i64::try_from(self.position).map_err(|_| ErrNo::EFBIG)?;
            copy_to_user_struct(self.user_ptr, &offset)
        }
    }
}

fn seek_errno(error : VfsError) -> ErrNo {
    match error {
        VfsError::Unsupported | VfsError::InvalidPath => ErrNo::ESPIPE,
        other => vfs_error_to_errno(other),
    }
}

fn validate_regular(fd : usize, write : bool) -> Result<(u64, u64), ErrNo> {
    vfs::fd::with_current_io(fd, |handle| {
        if write {
            validate_handle_write_access(handle)?;
        } else {
            handle.validate_read_access()?;
        }
        let metadata = handle.metadata()?;
        if metadata.node_type != VfsNodeType::File {
            return Err(VfsError::Unsupported);
        }
        if write && handle.open_status_flags() & O_APPEND != 0 {
            return Err(VfsError::BadFd);
        }
        Ok((metadata.mount_id, metadata.inode))
    }).map_err(|error| match error {
          VfsError::Unsupported => ErrNo::EINVAL,
          other => vfs_error_to_errno(other),
      })
}

fn validate_access(fd : usize, write : bool) -> Result<bool, ErrNo> {
    vfs::fd::with_current_io(fd, |handle| {
        if write {
            validate_handle_write_access(handle)?;
        } else {
            handle.validate_read_access()?;
        }
        Ok(handle.pipe_capacity()
                 .is_some())
    }).map_err(vfs_error_to_errno)
}

fn validate_handle_write_access(handle : &dyn vfs::api::VfsIoHandle) -> Result<(), VfsError> {
    const O_ACCMODE : u32 = 3;
    const O_RDONLY : u32 = 0;
    if handle.open_accmode() & O_ACCMODE == O_RDONLY {
        Err(VfsError::BadFd)
    } else {
        Ok(())
    }
}

fn ranges_overlap(first : u64, second : u64, len : usize) -> bool {
    if len == 0 {
        return false;
    }
    let extent = len as u64;
    let first_end = first.saturating_add(extent);
    let second_end = second.saturating_add(extent);
    first < second_end && second < first_end
}

fn finish_result(result : Result<usize, ErrNo>,
                 input : Option<(usize, FileCursor)>,
                 output : Option<(usize, FileCursor)>)
                 -> UserRet {
    let transferred = match result {
        Ok(transferred) => transferred,
        Err(error) => return UserRet::from_error(error),
    };
    if let Some((fd, cursor)) = input {
        if let Err(error) = cursor.publish(fd) {
            return UserRet::from_error(error);
        }
    }
    if let Some((fd, cursor)) = output {
        if let Err(error) = cursor.publish(fd) {
            return UserRet::from_error(error);
        }
    }
    UserRet::from_success(transferred)
}

fn read_regular(fd : usize, offset : u64, buf : &mut [u8]) -> Result<usize, ErrNo> {
    vfs::fd::with_current_io_detached(fd, |handle| handle.read_at(offset, buf))
        .map_err(vfs_io_at_error_to_errno)
}

fn write_at_all(fd : usize,
                mut offset : Option<u64>,
                buf : &[u8])
                -> Result<usize, (usize, ErrNo)> {
    let mut written = 0usize;
    while written < buf.len() {
        let result = vfs::fd::with_current_io_detached(fd, |handle| match offset {
            Some(position) => handle.write_at(position, &buf[written..]),
            None => handle.write(&buf[written..]),
        });
        match result {
            Ok(0) => break,
            Ok(bytes) => {
                written += bytes;
                if let Some(position) = &mut offset {
                    *position = match position.checked_add(bytes as u64) {
                        Some(next) => next,
                        None => return Err((written, ErrNo::EFBIG)),
                    };
                }
            }
            Err(error) => return Err((written, vfs_error_to_errno(error))),
        }
    }
    Ok(written)
}

fn signal_broken_pipe(error : Option<ErrNo>) {
    if error == Some(ErrNo::EPIPE) {
        let _ = crate::sys::ipc::signal::raise_current_thread(ipc::signal::SIGPIPE);
    }
}

/// `copy_file_range`：只接受两个普通文件，支持独立显式 offset。
pub(crate) fn sys_copy_file_range(args : SyscallArgs) -> UserRet {
    let input_fd = args.arg(0);
    let input_offset_ptr = args.arg(1);
    let output_fd = args.arg(2);
    let output_offset_ptr = args.arg(3);
    let requested = args.arg(4)
                        .min(MAX_RW_COUNT);
    let flags = args.arg(5);

    if flags != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let input_identity = match validate_regular(input_fd, false) {
        Ok(identity) => identity,
        Err(error) => return UserRet::from_error(error),
    };
    let output_identity = match validate_regular(output_fd, true) {
        Ok(identity) => identity,
        Err(error) => return UserRet::from_error(error),
    };
    let mut input = match FileCursor::import(input_fd, input_offset_ptr) {
        Ok(cursor) => cursor,
        Err(error) => return UserRet::from_error(error),
    };
    let mut output = match FileCursor::import(output_fd, output_offset_ptr) {
        Ok(cursor) => cursor,
        Err(error) => return UserRet::from_error(error),
    };
    if input_identity == output_identity &&
       ranges_overlap(input.position,
                      output.position,
                      requested)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if requested == 0 {
        return UserRet::from_success(0);
    }

    let mut buffer = match try_kbuf(requested.min(IO_CHUNK), IO_CHUNK) {
        Ok(buffer) => buffer,
        Err(error) => return UserRet::from_error(error),
    };
    let mut transferred = 0usize;
    let mut terminal_error = None;
    while transferred < requested {
        let chunk = (requested - transferred).min(buffer.len());
        let read = match read_regular(input_fd,
                                      input.position,
                                      &mut buffer[..chunk])
        {
            Ok(read) => read,
            Err(error) => {
                terminal_error = Some(error);
                break;
            }
        };
        if read == 0 {
            break;
        }
        match write_at_all(output_fd,
                           Some(output.position),
                           &buffer[..read])
        {
            Ok(written) => {
                if let Err(error) = input.advance(written)
                                         .and_then(|_| output.advance(written))
                {
                    terminal_error = Some(error);
                    break;
                }
                transferred += written;
                if written < read {
                    break;
                }
            }
            Err((written, error)) => {
                let advance = input.advance(written)
                                   .and_then(|_| output.advance(written));
                transferred += written;
                terminal_error = Some(advance.err()
                                             .unwrap_or(error));
                break;
            }
        }
    }
    let result = if transferred > 0 || terminal_error.is_none() {
        Ok(transferred)
    } else {
        Err(terminal_error.unwrap())
    };
    finish_result(result,
                  Some((input_fd, input)),
                  Some((output_fd, output)))
}

fn acquire_pipe_read(fd : usize,
                     max_len : usize)
                     -> Result<alloc::boxed::Box<dyn VfsReadLease>, ErrNo> {
    loop {
        let prepared = vfs::fd::prepare_current_read(fd, max_len).map_err(vfs_error_to_errno)?;
        match prepared.acquire() {
            Ok(lease) => return Ok(lease),
            Err(VfsError::Busy) => task::yield_now(),
            Err(error) => return Err(vfs_error_to_errno(error)),
        }
    }
}

fn nonblocking_pipe_limit(fd : usize, write : bool) -> Result<usize, ErrNo> {
    vfs::fd::with_current_io(fd, |handle| {
        let capacity = handle.pipe_capacity()
                             .ok_or(VfsError::BadFd)?;
        let used = handle.pipe_buffer_len()
                         .ok_or(VfsError::BadFd)?;
        Ok(if write {
            capacity.saturating_sub(used)
        } else {
            used
        })
    }).map_err(vfs_error_to_errno)
}

/// `splice`：至少一端必须是 pipe；文件端使用 `read_at/write_at`，pipe 输入
/// 通过 read lease 延迟消费，因而可以正确处理短写。
pub(crate) fn sys_splice(args : SyscallArgs) -> UserRet {
    let input_fd = args.arg(0);
    let input_offset_ptr = args.arg(1);
    let output_fd = args.arg(2);
    let output_offset_ptr = args.arg(3);
    let mut requested = args.arg(4)
                            .min(MAX_RW_COUNT);
    let flags = args.arg(5);

    if flags & !SPLICE_FLAGS != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let input_pipe = match validate_access(input_fd, false) {
        Ok(pipe) => pipe,
        Err(error) => return UserRet::from_error(error),
    };
    let output_pipe = match validate_access(output_fd, true) {
        Ok(pipe) => pipe,
        Err(error) => return UserRet::from_error(error),
    };
    if !input_pipe && !output_pipe {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if (input_pipe && input_offset_ptr != 0) || (output_pipe && output_offset_ptr != 0) {
        return UserRet::from_error(ErrNo::ESPIPE);
    }
    if !input_pipe {
        if let Err(error) = validate_regular(input_fd, false) {
            return UserRet::from_error(error);
        }
    }
    if !output_pipe {
        if let Err(error) = validate_regular(output_fd, true) {
            return UserRet::from_error(error);
        }
    }
    let mut input = if input_pipe {
        None
    } else {
        match FileCursor::import(input_fd, input_offset_ptr) {
            Ok(cursor) => Some(cursor),
            Err(error) => return UserRet::from_error(error),
        }
    };
    let mut output = if output_pipe {
        None
    } else {
        match FileCursor::import(output_fd, output_offset_ptr) {
            Ok(cursor) => Some(cursor),
            Err(error) => return UserRet::from_error(error),
        }
    };
    if requested == 0 {
        return UserRet::from_success(0);
    }
    if flags & SPLICE_F_NONBLOCK != 0 {
        if input_pipe {
            requested = requested.min(match nonblocking_pipe_limit(input_fd, false) {
                                          Ok(0) => return UserRet::from_error(ErrNo::EAGAIN),
                                          Ok(bytes) => bytes,
                                          Err(error) => return UserRet::from_error(error),
                                      });
        }
        if output_pipe {
            requested = requested.min(match nonblocking_pipe_limit(output_fd, true) {
                                          Ok(0) => return UserRet::from_error(ErrNo::EAGAIN),
                                          Ok(bytes) => bytes,
                                          Err(error) => return UserRet::from_error(error),
                                      });
        }
    }

    let mut buffer = if input_pipe {
        Vec::new()
    } else {
        match try_kbuf(requested.min(IO_CHUNK), IO_CHUNK) {
            Ok(buffer) => buffer,
            Err(error) => return UserRet::from_error(error),
        }
    };
    let mut transferred = 0usize;
    let mut terminal_error = None;
    while transferred < requested {
        let chunk = (requested - transferred).min(IO_CHUNK);
        if input_pipe {
            let lease = match acquire_pipe_read(input_fd, chunk) {
                Ok(lease) => lease,
                Err(error) => {
                    terminal_error = Some(error);
                    break;
                }
            };
            if lease.bytes()
                    .is_empty()
            {
                let _ = lease.finish(VfsCopyProgress { copied : 0,
                                                       complete : true });
                break;
            }
            let write_offset = output.map(|cursor| cursor.position);
            let (written, write_error) = match write_at_all(output_fd, write_offset, lease.bytes())
            {
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
            if let Some(cursor) = &mut output {
                if let Err(error) = cursor.advance(written) {
                    terminal_error = Some(error);
                    break;
                }
            }
            transferred += written;
            if let Some(error) = write_error {
                terminal_error = Some(error);
                break;
            }
            if written == 0 {
                break;
            }
        } else {
            let cursor = input.as_mut()
                              .expect("regular splice input has cursor");
            let read = match read_regular(input_fd,
                                          cursor.position,
                                          &mut buffer[..chunk])
            {
                Ok(read) => read,
                Err(error) => {
                    terminal_error = Some(error);
                    break;
                }
            };
            if read == 0 {
                break;
            }
            let write_offset = output.map(|cursor| cursor.position);
            let (written, write_error) =
                match write_at_all(output_fd, write_offset, &buffer[..read]) {
                    Ok(written) => (written, None),
                    Err((written, error)) => (written, Some(error)),
                };
            if let Err(error) = cursor.advance(written) {
                terminal_error = Some(error);
                break;
            }
            if let Some(output_cursor) = &mut output {
                if let Err(error) = output_cursor.advance(written) {
                    terminal_error = Some(error);
                    break;
                }
            }
            transferred += written;
            if let Some(error) = write_error {
                terminal_error = Some(error);
                break;
            }
            if written < read {
                break;
            }
        }
    }
    let result = if transferred > 0 || terminal_error.is_none() {
        Ok(transferred)
    } else {
        Err(terminal_error.unwrap())
    };
    signal_broken_pipe(terminal_error);
    finish_result(result,
                  input.map(|cursor| (input_fd, cursor)),
                  output.map(|cursor| (output_fd, cursor)))
}

/// `tee`：把输入 pipe 当前可见前缀复制到输出 pipe，但以 `copied=0` 完成
/// 输入 read lease，因此源数据保持不变。
pub(crate) fn sys_tee(args : SyscallArgs) -> UserRet {
    let input_fd = args.arg(0);
    let output_fd = args.arg(1);
    let mut requested = args.arg(2).min(MAX_RW_COUNT);
    let flags = args.arg(3);
    if flags & !SPLICE_FLAGS != 0 || input_fd == output_fd {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let input_pipe = match validate_access(input_fd, false) {
        Ok(pipe) => pipe,
        Err(error) => return UserRet::from_error(error),
    };
    let output_pipe = match validate_access(output_fd, true) {
        Ok(pipe) => pipe,
        Err(error) => return UserRet::from_error(error),
    };
    if !input_pipe || !output_pipe {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let same_pipe = vfs::fd::with_current_io(input_fd, |handle| handle.metadata())
        .and_then(|input| {
            vfs::fd::with_current_io(output_fd, |handle| {
                handle.metadata().map(|output| input.inode == output.inode)
            })
        });
    if same_pipe == Ok(true) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if requested == 0 {
        return UserRet::from_success(0);
    }
    if flags & SPLICE_F_NONBLOCK != 0 {
        requested = requested.min(match nonblocking_pipe_limit(input_fd, false) {
                                      Ok(0) => return UserRet::from_error(ErrNo::EAGAIN),
                                      Ok(bytes) => bytes,
                                      Err(error) => return UserRet::from_error(error),
                                  });
        requested = requested.min(match nonblocking_pipe_limit(output_fd, true) {
                                      Ok(0) => return UserRet::from_error(ErrNo::EAGAIN),
                                      Ok(bytes) => bytes,
                                      Err(error) => return UserRet::from_error(error),
                                  });
    }
    let lease = match acquire_pipe_read(input_fd, requested) {
        Ok(lease) => lease,
        Err(error) => return UserRet::from_error(error),
    };
    if lease.bytes().is_empty() {
        let _ = lease.finish(VfsCopyProgress { copied : 0,
                                               complete : true });
        return UserRet::from_success(0);
    }
    let (written, write_error) = match write_at_all(output_fd, None, lease.bytes()) {
        Ok(written) => (written, None),
        Err((written, error)) => (written, Some(error)),
    };
    let finish = lease.finish(VfsCopyProgress { copied : 0,
                                                complete : true });
    if let Err(error) = finish {
        return UserRet::from_error(vfs_error_to_errno(error));
    }
    signal_broken_pipe(write_error);
    if written > 0 || write_error.is_none() {
        UserRet::from_success(written)
    } else {
        UserRet::from_error(write_error.unwrap())
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base : usize,
    len : usize,
}

/// `vmsplice`：首版将用户 iovec 内容复制到 pipe。Linux 的 `SPLICE_F_GIFT`
/// 只是一项所有权提示；WaterOS 不把用户页直接赠送给 pipe，所以可安全忽略该提示。
pub(crate) fn sys_vmsplice(args : SyscallArgs) -> UserRet {
    let output_fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iov_count = args.arg(2);
    let flags = args.arg(3);
    if flags & !SPLICE_FLAGS != 0 || iov_count > IOV_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if iov_count != 0 && iov_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    match validate_access(output_fd, true) {
        Ok(true) => {}
        Ok(false) => return UserRet::from_error(ErrNo::EINVAL),
        Err(error) => return UserRet::from_error(error),
    }
    let call_limit = if flags & SPLICE_F_NONBLOCK != 0 {
        match nonblocking_pipe_limit(output_fd, true) {
            Ok(0) => return UserRet::from_error(ErrNo::EAGAIN),
            Ok(bytes) => bytes.min(MAX_RW_COUNT),
            Err(error) => return UserRet::from_error(error),
        }
    } else {
        MAX_RW_COUNT
    };
    let mut transferred = 0usize;
    let mut buffer = match try_kbuf(IO_CHUNK, IO_CHUNK) {
        Ok(buffer) => buffer,
        Err(error) => return UserRet::from_error(error),
    };
    for index in 0..iov_count {
        let address = match index.checked_mul(core::mem::size_of::<UserIoVec>())
                                 .and_then(|offset| iov_ptr.checked_add(offset)) {
            Some(address) => address,
            None => return if transferred > 0 {
                UserRet::from_success(transferred)
            } else {
                UserRet::from_error(ErrNo::EFAULT)
            },
        };
        let iov = match copy_from_user_struct::<UserIoVec>(address) {
            Ok(iov) => iov,
            Err(error) => return if transferred > 0 {
                UserRet::from_success(transferred)
            } else {
                UserRet::from_error(error)
            },
        };
        if iov.len != 0 && iov.base == 0 {
            return if transferred > 0 {
                UserRet::from_success(transferred)
            } else {
                UserRet::from_error(ErrNo::EFAULT)
            };
        }
        let allowed = call_limit.saturating_sub(transferred);
        let mut iov_done = 0usize;
        while iov_done < iov.len.min(allowed) {
            let chunk = (iov.len.min(allowed) - iov_done).min(buffer.len());
            let source = match iov.base.checked_add(iov_done) {
                Some(source) => source,
                None => return UserRet::from_success(transferred),
            };
            match copy_from_user(&mut buffer[..chunk], source) {
                Ok(copied) if copied == chunk => {}
                Ok(_) | Err(_) => return if transferred > 0 {
                    UserRet::from_success(transferred)
                } else {
                    UserRet::from_error(ErrNo::EFAULT)
                },
            }
            let (written, write_error) = match write_at_all(output_fd, None, &buffer[..chunk]) {
                Ok(written) => (written, None),
                Err((written, error)) => (written, Some(error)),
            };
            transferred += written;
            iov_done += written;
            if written < chunk || write_error.is_some() {
                signal_broken_pipe(write_error);
                return if transferred > 0 {
                    UserRet::from_success(transferred)
                } else {
                    UserRet::from_error(write_error.unwrap_or(ErrNo::EIO))
                };
            }
        }
        if transferred == call_limit {
            break;
        }
    }
    UserRet::from_success(transferred)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_half_open() {
        assert!(ranges_overlap(10, 15, 10));
        assert!(!ranges_overlap(10, 20, 10));
        assert!(!ranges_overlap(10, 10, 0));
    }

    #[test]
    fn cursor_overflow_is_rejected() {
        let mut cursor = FileCursor { user_ptr : 0,
                                      position : u64::MAX };
        assert_eq!(cursor.advance(1), Err(ErrNo::EFBIG));
    }
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    assert!(ranges_overlap(10, 15, 10));
    assert!(!ranges_overlap(10, 20, 10));
    let mut cursor = FileCursor { user_ptr : 0,
                                  position : u64::MAX };
    assert_eq!(cursor.advance(1), Err(ErrNo::EFBIG));
}
