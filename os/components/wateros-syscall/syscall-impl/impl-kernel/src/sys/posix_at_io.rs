//! `pread64` / `pwrite64` / `preadv` / `pwritev`：按绝对文件偏移读写，不改变 fd 当前偏移。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user};
use crate::vfs_util::vfs_io_at_error_to_errno;

const MAX_IO: usize = 4 * 1024 * 1024;
const IO_CHUNK: usize = 64 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base: usize,
    len:  usize,
}

fn offset_from_arg(raw: usize) -> Result<u64, ErrNo> {
    let off = raw as i64;
    if off < 0 {
        return Err(ErrNo::EINVAL);
    }
    Ok(off as u64)
}

fn gather_user_iovecs(iov_ptr: usize, iovcnt: usize) -> Result<Vec<u8>, ErrNo> {
    if iovcnt == 0 {
        return Ok(Vec::new());
    }
    if iov_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if iovcnt > 1024 {
        return Err(ErrNo::EINVAL);
    }

    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut out = Vec::new();
    for i in 0..iovcnt {
        let iov = copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size)?;
        if iov.len == 0 {
            continue;
        }
        if iov.base == 0 {
            return Err(ErrNo::EFAULT);
        }
        let new_len = out
            .len()
            .checked_add(iov.len)
            .ok_or(ErrNo::EINVAL)?;
        if new_len > MAX_IO {
            return Err(ErrNo::EINVAL);
        }
        let old_len = out.len();
        out.resize(new_len, 0);
        match copy_from_user(&mut out[old_len..], iov.base) {
            Ok(n) if n == iov.len => {}
            _ => return Err(ErrNo::EFAULT),
        }
    }
    Ok(out)
}

fn scatter_to_user_iovecs(iov_ptr: usize, iovcnt: usize, data: &[u8]) -> Result<usize, ErrNo> {
    if data.is_empty() {
        return Ok(0);
    }
    if iovcnt == 0 {
        return Ok(0);
    }
    if iov_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }

    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut written = 0usize;
    let mut src_off = 0usize;
    for i in 0..iovcnt {
        if src_off >= data.len() {
            break;
        }
        let iov = copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size)?;
        if iov.len == 0 {
            continue;
        }
        if iov.base == 0 {
            return Err(ErrNo::EFAULT);
        }
        let n = iov.len.min(data.len() - src_off);
        copy_to_user(iov.base, &data[src_off..src_off + n])?;
        src_off += n;
        written += n;
    }
    Ok(written)
}

fn total_iov_len(iov_ptr: usize, iovcnt: usize) -> Result<usize, ErrNo> {
    if iovcnt == 0 {
        return Ok(0);
    }
    if iov_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if iovcnt > 1024 {
        return Err(ErrNo::EINVAL);
    }
    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut total = 0usize;
    for i in 0..iovcnt {
        let iov = copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size)?;
        total = total
            .checked_add(iov.len)
            .ok_or(ErrNo::EINVAL)?;
        if total > MAX_IO {
            return Err(ErrNo::EINVAL);
        }
    }
    Ok(total)
}

pub(crate) fn sys_pread64(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > MAX_IO {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    let mut kbuf = Vec::with_capacity(len);
    kbuf.resize(len, 0);
    let n = match vfs::fd::with_current_io(fd, |handle| handle.read_at(offset, &mut kbuf)) {
        Ok(n) => n,
        Err(err) => return UserRet::from_error(vfs_io_at_error_to_errno(err)),
    };
    if n == 0 {
        return UserRet::from_success(0);
    }
    match copy_to_user(ptr, &kbuf[..n]) {
        Ok(w) if w == n => UserRet::from_success(n),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}

pub(crate) fn sys_pwrite64(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > MAX_IO {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    let mut kbuf = Vec::with_capacity(len);
    kbuf.resize(len, 0);
    match copy_from_user(&mut kbuf, ptr) {
        Ok(n) if n == len => {}
        _ => return UserRet::from_error(ErrNo::EFAULT),
    }
    match vfs::fd::with_current_io(fd, |handle| handle.write_at(offset, &kbuf)) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(vfs_io_at_error_to_errno(err)),
    }
}

pub(crate) fn sys_preadv(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    let want = match total_iov_len(iov_ptr, iovcnt) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    if want == 0 {
        return UserRet::from_success(0);
    }
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    let mut file_off = offset;
    let mut gathered = Vec::new();
    let mut remaining = want;
    while remaining > 0 {
        let chunk = remaining.min(IO_CHUNK);
        let mut kbuf = Vec::new();
        kbuf.resize(chunk, 0);
        let n = match vfs::fd::with_current_io(fd, |handle| handle.read_at(file_off, &mut kbuf)) {
            Ok(n) => n,
            Err(err) => return UserRet::from_error(vfs_io_at_error_to_errno(err)),
        };
        if n == 0 {
            break;
        }
        gathered.extend_from_slice(&kbuf[..n]);
        file_off = match file_off.checked_add(n as u64) {
            Some(v) => v,
            None => return UserRet::from_error(ErrNo::EINVAL),
        };
        remaining -= n;
    }

    let scattered = match scatter_to_user_iovecs(iov_ptr, iovcnt, &gathered) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    UserRet::from_success(scattered)
}

pub(crate) fn sys_pwritev(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    let data = match gather_user_iovecs(iov_ptr, iovcnt) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    if data.is_empty() {
        return UserRet::from_success(0);
    }
    match vfs::fd::with_current_io(fd, |handle| handle.write_at(offset, &data)) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(vfs_io_at_error_to_errno(err)),
    }
}
