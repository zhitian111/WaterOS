//! Syscall 内核缓冲的可失败分配，避免 OOM 时触发全局 `alloc_error_handler`。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::vec::Vec;

use api_v0::ErrNo;

/// 与 `read`/`write`/`pread` 族 syscall 对齐的上限。
// 本变量代码由AI完成
pub const SYSCALL_IO_MAX : usize = 4 * 1024 * 1024;

/// socket option 值的防御性上限。
// 本变量代码由AI完成
pub const SYSCALL_SOCK_IO_MAX : usize = 64 * 1024;

/// `sched_getaffinity` / `sched_setaffinity` 用户缓冲上界。
pub const SCHED_CPUSET_MAX : usize = 4096;

/// `getdents64` 单次用户缓冲合理上界。
pub const GETDENTS64_MAX : usize = 256 * 1024;

/// 分配长度为 `len` 的零初始化缓冲；`len > max` 返回 `EINVAL`，堆不足返回 `ENOMEM`。
// 本方法代码由AI完成
pub fn try_kbuf(len : usize, max : usize) -> Result<Vec<u8>, ErrNo> {
    if len > max {
        return Err(ErrNo::EINVAL);
    }
    if len == 0 {
        return Ok(Vec::new());
    }
    let mut buf = Vec::new();
    buf.try_reserve_exact(len)
       .map_err(|_| ErrNo::ENOMEM)?;
    buf.resize(len, 0);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_kbuf_rejects_over_max() {
        assert_eq!(try_kbuf(GETDENTS64_MAX + 1, GETDENTS64_MAX),
                   Err(ErrNo::EINVAL));
    }

    #[test]
    fn try_kbuf_zero_len() {
        assert!(try_kbuf(0, GETDENTS64_MAX).unwrap().is_empty());
    }
}
