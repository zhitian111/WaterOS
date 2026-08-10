//! 打开态文件句柄（fd 会话的上游抽象；完整实现见后续工作包）。

extern crate alloc;

use alloc::boxed::Box;
use core::any::Any;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{VfsError, VfsResult};
use crate::meta::VfsMetadata;

/// One Linux open-file-description's shared seek position and status flags.
///
/// Each successful `open` creates a new state. `dup` and `fork` wrappers share
/// it through an `Arc`; descriptor flags such as `FD_CLOEXEC` do not belong
/// here. Slow backing I/O must not execute while holding an OFD spin lock, so
/// these scalar fields use atomics. RIO-04 uses `next_reservation_generation`
/// to order prepared read reservations.
pub struct VfsOpenDescriptionState {
    offset : AtomicU64,
    status_flags : AtomicU32,
    reservation_generation : AtomicU64,
    read_reservation : Mutex<Option<VfsReadReservation>>,
}

impl VfsOpenDescriptionState {
    pub const fn new(offset : u64, status_flags : u32) -> Self {
        Self { offset : AtomicU64::new(offset),
               status_flags : AtomicU32::new(status_flags),
               reservation_generation : AtomicU64::new(1),
               read_reservation : Mutex::new(None) }
    }

    #[inline]
    pub fn offset(&self) -> u64 { self.offset.load(Ordering::Acquire) }

    #[inline]
    pub fn set_offset(&self, offset : u64) { self.offset.store(offset, Ordering::Release); }

    /// Atomically add a completed sequential I/O length to the shared offset.
    pub fn advance_offset(&self, amount : u64) -> VfsResult<u64> {
        self.offset
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |offset| {
                offset.checked_add(amount)
            })
            .map(|old| old + amount)
            .map_err(|_| VfsError::Io)
    }

    /// Atomically apply `SEEK_CUR` style signed displacement.
    pub fn add_signed_offset(&self, displacement : i64) -> VfsResult<u64> {
        self.offset
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |offset| {
                if displacement < 0 {
                    offset.checked_sub(displacement.unsigned_abs())
                } else {
                    offset.checked_add(displacement as u64)
                }
            })
            .map(|old| {
                if displacement < 0 {
                    old - displacement.unsigned_abs()
                } else {
                    old + displacement as u64
                }
            })
            .map_err(|_| VfsError::InvalidPath)
    }

    #[inline]
    pub fn status_flags(&self) -> u32 { self.status_flags.load(Ordering::Acquire) }

    #[inline]
    pub fn set_status_flags(&self, flags : u32) {
        self.status_flags.store(flags, Ordering::Release);
    }

    /// Allocate a monotonically increasing id for a future prepared read.
    #[inline]
    pub fn next_reservation_generation(&self) -> u64 {
        self.reservation_generation.fetch_add(1, Ordering::AcqRel)
    }

    /// Reserve the current sequential offset for a prepared read.
    pub fn begin_read(&self) -> VfsResult<VfsReadReservation> {
        let mut active = self.read_reservation.lock();
        if active.is_some() {
            return Err(VfsError::Busy);
        }
        let reservation =
            VfsReadReservation { id : self.next_reservation_generation(),
                                 offset : self.offset() };
        *active = Some(reservation);
        Ok(reservation)
    }

    /// Change the captured position while retaining the same active reservation.
    pub fn retarget_read(&self,
                         reservation : VfsReadReservation,
                         offset : u64)
                         -> VfsResult<VfsReadReservation> {
        let mut active = self.read_reservation.lock();
        if active.as_ref().map(|entry| entry.id) != Some(reservation.id) {
            return Err(VfsError::Io);
        }
        let updated = VfsReadReservation { id : reservation.id,
                                           offset };
        *active = Some(updated);
        Ok(updated)
    }

    /// Commit only bytes that reached userspace, then release the reservation.
    pub fn finish_read(&self,
                       reservation : VfsReadReservation,
                       copied : usize,
                       staged : usize)
                       -> VfsResult<u64> {
        let mut active = self.read_reservation.lock();
        if active.as_ref().map(|entry| entry.id) != Some(reservation.id) {
            return Err(VfsError::Io);
        }
        let new_offset = if copied <= staged {
            reservation.offset.checked_add(copied as u64)
        } else {
            None
        };
        *active = None;
        let new_offset = new_offset.ok_or(VfsError::Io)?;
        self.offset.store(new_offset, Ordering::Release);
        Ok(new_offset)
    }

    /// Complete a reserved sequential operation at an explicitly chosen offset.
    pub fn finish_read_at(&self,
                          reservation : VfsReadReservation,
                          new_offset : u64)
                          -> VfsResult<u64> {
        let mut active = self.read_reservation.lock();
        if active.as_ref().map(|entry| entry.id) != Some(reservation.id) {
            return Err(VfsError::Io);
        }
        *active = None;
        self.offset.store(new_offset, Ordering::Release);
        Ok(new_offset)
    }

    /// Cancel a prepared read without changing the shared offset.
    pub fn cancel_read(&self, reservation : VfsReadReservation) -> VfsResult<()> {
        let mut active = self.read_reservation.lock();
        if active.as_ref().map(|entry| entry.id) != Some(reservation.id) {
            return Err(VfsError::Io);
        }
        *active = None;
        Ok(())
    }

    #[inline]
    pub fn read_reservation_active(&self) -> bool { self.read_reservation.lock().is_some() }

    pub fn set_offset_if_idle(&self, offset : u64) -> VfsResult<u64> {
        let active = self.read_reservation.lock();
        if active.is_some() {
            return Err(VfsError::Busy);
        }
        self.offset.store(offset, Ordering::Release);
        Ok(offset)
    }

    pub fn add_signed_offset_if_idle(&self, displacement : i64) -> VfsResult<u64> {
        let active = self.read_reservation.lock();
        if active.is_some() {
            return Err(VfsError::Busy);
        }
        self.add_signed_offset(displacement)
    }

    pub fn clamp_offset_if_idle(&self, maximum : u64) -> VfsResult<u64> {
        let active = self.read_reservation.lock();
        if active.is_some() {
            return Err(VfsError::Busy);
        }
        let offset = self.offset().min(maximum);
        self.offset.store(offset, Ordering::Release);
        Ok(offset)
    }
}

/// Identity and captured offset of one active sequential read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VfsReadReservation {
    id : u64,
    offset : u64,
}

impl VfsReadReservation {
    #[inline]
    pub const fn offset(self) -> u64 { self.offset }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VfsCopyProgress {
    pub copied : usize,
    pub complete : bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsReadFinish {
    Bytes(usize),
    Fault,
}

/// Stable staged data whose source position is committed only by `finish`.
pub trait VfsReadLease: Send {
    fn bytes(&self) -> &[u8];

    fn finish(self : Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish>;
}

/// Owned read operation produced while the fd slot lock is held briefly.
pub trait VfsPreparedRead: Send {
    fn acquire(self : Box<Self>) -> VfsResult<Box<dyn VfsReadLease>>;
}

/// `open` 标志位（占位；与 ABI `O_*` 对齐将随 syscall 工作包演进）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VfsOpenFlags(pub u32);

impl VfsOpenFlags {
    /// 语义层读（与 Linux `O_RDONLY` 对应，Linux 值为 0）。
    pub const READ: u32 = 1;
    /// 语义层写（对应 `O_WRONLY` / `O_RDWR` 的写侧）。
    pub const WRITE: u32 = 2;
    /// 不存在则创建（`O_CREAT`）。
    pub const CREATE: u32 = 4;
    /// 打开时截断（`O_TRUNC`）。
    pub const TRUNC: u32 = 8;
    /// 追加写（`O_APPEND`）。
    pub const APPEND: u32 = 16;
    /// 仅打开目录（`O_DIRECTORY`）。
    pub const DIRECTORY: u32 = 32;
    /// 打开本身以及后续 I/O 均不得阻塞（`O_NONBLOCK`）。
    pub const NONBLOCK: u32 = 64;

    /// 只读打开标志组合。
    pub const fn read() -> Self {
        Self(Self::READ)
    }

    /// 是否包含指定标志位。
    pub fn contains(&self, flag: u32) -> bool {
        self.0 & flag != 0
    }
}

/// 为 fd 句柄提供对象安全的类型识别，供 syscall 层从统一 VFS fd 表中
/// 识别 socket、epoll 等特殊句柄。
pub trait VfsHandleAny {
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any> VfsHandleAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 流式读写的已打开对象（pipe、控制台、文件会话等）。
pub trait VfsIoHandle: Send + VfsHandleAny {
    /// Capture owned state for a sequential read without waiting or doing I/O.
    fn prepare_read(&mut self, _max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Err(VfsError::Unsupported)
    }

    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let _ = buf;
        Err(VfsError::Unsupported)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        let _ = buf;
        Err(VfsError::Unsupported)
    }

    fn close(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Err(VfsError::Unsupported)
    }

    fn seek(&mut self, _offset: i64, _whence: VfsSeekWhence) -> VfsResult<u64> {
        Err(VfsError::Unsupported)
    }

    /// 在绝对文件偏移处读（`pread` 族）；默认不支持（pipe/socket 等）。
    /// 不得修改顺序 `read` 使用的当前偏移。
    fn read_at(&mut self, _offset: u64, _buf: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::Unsupported)
    }

    /// 在绝对文件偏移处写（`pwrite` 族）；默认不支持。
    fn write_at(&mut self, _offset: u64, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::Unsupported)
    }

    fn flush(&mut self) -> VfsResult<()> {
        Err(VfsError::Unsupported)
    }

    fn truncate(&mut self, _len: u64) -> VfsResult<()> {
        Err(VfsError::Unsupported)
    }

    /// 若本句柄表示已打开目录，返回其绝对路径（供 `openat(dirfd, …)` 解析相对路径）。
    fn directory_path(&self) -> Option<&str> {
        None
    }

    /// 若本句柄对应路径型文件/目录，返回其绝对路径（供 `f*xattr` 等使用）。
    fn backing_path(&self) -> Option<&str> {
        None
    }

    /// `flock(2)` 的打开文件描述 owner；`dup`/`fork` 复制时应保持不变。
    fn flock_owner_id(&self) -> Option<u64> {
        None
    }

    /// 将目录项写入 `buf`（`getdents64` 布局）；非目录句柄默认 [`VfsError::Unsupported`]。
    fn fill_getdents64(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let _ = buf;
        Err(VfsError::Unsupported)
    }

    /// 复制本句柄为新的独立 fd 对象（`dup` / `fork` 继承）。
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Err(VfsError::Unsupported)
    }

    fn ioctl(&mut self, _request: usize, _arg: usize) -> VfsResult<isize> {
        Err(VfsError::Unsupported)
    }

    /// 是否为软件 RTC 字符设备（syscall 层对 rtc fd 分发专用 ioctl）。
    fn is_rtc_device(&self) -> bool {
        false
    }

    /// 是否为 TTY 类字符设备（UART/console 等；syscall 层分发 TTY ioctl）。
    fn is_tty_char_device(&self) -> bool {
        false
    }

    /// 若为 `/dev/input/eventN`，返回稳定的输入设备注册索引。
    fn input_event_index(&self) -> Option<usize> {
        None
    }

    /// `poll`/`ppoll` 就绪位查询；默认不支持（`POLLNVAL` 由 syscall 层处理）。
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        let _ = events;
        Err(VfsError::Unsupported)
    }

    /// Linux `fcntl(F_GETFL)` 状态位（不含 `O_ACCMODE`）；默认无 `O_NONBLOCK`。
    fn open_status_flags(&self) -> u32 {
        0
    }

    /// 在不执行 I/O 的情况下验证句柄是否允许 `read(2)`。
    fn validate_read_access(&self) -> VfsResult<()> {
        const O_ACCMODE: u32 = 3;
        const O_WRONLY: u32 = 1;
        if self.open_accmode() & O_ACCMODE == O_WRONLY {
            Err(VfsError::BadFd)
        } else {
            Ok(())
        }
    }

    /// Linux `fcntl(F_GETFL)` 访问模式（`O_ACCMODE`：0/1/2）。
    ///
    /// 访问模式没有安全的默认值；每类句柄必须明确声明。
    fn open_accmode(&self) -> u32;

    /// Linux `fcntl(F_SETFL)`；默认忽略（pipe/socket 等由具体实现覆盖）。
    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        let _ = flags;
        Ok(())
    }

    /// 在句柄对应 waitqueue 上阻塞；`still_waiting` 为假时返回（多 fd poll 重扫）。
    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
        let _ = (events, timeout_ticks, still_waiting);
        Err(VfsError::Unsupported)
    }

    /// pipe fd：`F_GETPIPE_SZ` 返回容量；非 pipe 返回 `None`。
    fn pipe_capacity(&self) -> Option<usize> {
        None
    }

    /// pipe fd：返回当前已缓冲字节数；非 pipe 返回 `None`。
    fn pipe_buffer_len(&self) -> Option<usize> {
        None
    }

    /// pipe fd：`F_SETPIPE_SZ` 调整容量；非 pipe 返回 [`VfsError::Unsupported`]。
    fn pipe_set_capacity(&mut self, _capacity: usize) -> VfsResult<usize> {
        Err(VfsError::Unsupported)
    }
}

/// 已打开文件的偏移读写（由 [`VfsOpenOps`] 创建）；`read_at`/`write_at` 见 [`VfsIoHandle`]。
pub trait VfsFileHandle: VfsIoHandle {
    fn seek(&mut self, offset: i64, whence: VfsSeekWhence) -> VfsResult<u64> {
        let _ = (offset, whence);
        Err(VfsError::Unsupported)
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Err(VfsError::Unsupported)
    }
}

/// `lseek` / `seek` 基准：绝对偏移、相对当前、相对 EOF。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsSeekWhence {
    /// `SEEK_SET`：相对文件开头。
    Set,
    /// `SEEK_CUR`：相对当前偏移。
    Cur,
    /// `SEEK_END`：相对文件末尾。
    End,
}

/// 路径级打开为句柄。
pub trait VfsOpenOps {
    /// 按绝对路径与标志打开；返回的对象经 fd 会话分配编号后供 syscall 使用。
    fn open(&self, path: &str, flags: VfsOpenFlags) -> VfsResult<Box<dyn VfsIoHandle>> {
        let _ = (path, flags);
        Err(VfsError::Unsupported)
    }
}
