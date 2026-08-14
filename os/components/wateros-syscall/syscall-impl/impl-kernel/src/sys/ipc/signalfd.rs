//! `signalfd4(2)`：把已阻塞的 pending signal 作为可读 fd 记录消费。

extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};

use api_v0::{ErrNo, SyscallArgs, UserRet};
use ipc::signal::{SignalSet, TakenPendingSignal, SIGKILL, SIGSTOP};
use spin::Mutex;
use vfs::{
    api::{
        VfsCopyProgress, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsPreparedRead,
        VfsReadFinish, VfsReadLease, VfsResult,
    },
    fd,
};

use crate::{
    sys::ipc::signal::{peek_pending_signal_source, take_pending_signal_source},
    user_copy::copy_from_user_struct,
    vfs_util::vfs_error_to_errno,
};

const SFD_NONBLOCK : usize = 0o0004000;
const SFD_CLOEXEC : usize = 0o2000000;
const FD_CLOEXEC : usize = 1;
const POLLIN : i16 = 0x001;
const SIGNALFD_SIGINFO_SIZE : usize = 128;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SignalFdSigInfo {
    signo : u32,
    errno : i32,
    code : i32,
    pid : u32,
    uid : u32,
    fd : i32,
    tid : u32,
    band : u32,
    overrun : u32,
    trapno : u32,
    status : i32,
    int_value : i32,
    ptr : u64,
    utime : u64,
    stime : u64,
    addr : u64,
    addr_lsb : u16,
    pad2 : u16,
    syscall : i32,
    call_addr : u64,
    arch : u32,
    pad : [u8; 28],
}

const _ : () = assert!(core::mem::size_of::<SignalFdSigInfo>() == SIGNALFD_SIGINFO_SIZE);

struct SignalFdInner {
    mask : SignalSet,
    nonblocking : bool,
}

struct SignalFdState {
    inner : Mutex<SignalFdInner>,
    wait : task::wait_queue::WaitQueue,
}

impl SignalFdState {
    fn new(mask : SignalSet, nonblocking : bool) -> Self {
        Self { inner : Mutex::new(SignalFdInner { mask, nonblocking }),
               wait : task::wait_queue::WaitQueue::new_named("signalfd") }
    }

    fn mask(&self) -> SignalSet { self.inner.lock().mask }

    fn pending_for(&self, task_id : usize) -> bool {
        ipc::signal::pending_in(task_id, self.mask()).unwrap_or(false)
    }

    fn update_mask(&self, mask : SignalSet) {
        self.inner.lock().mask = mask;
        self.wait.wake_all();
    }
}

impl Drop for SignalFdState {
    fn drop(&mut self) {
        self.wait.wake_all();
        let _ = self.wait.try_release_empty();
    }
}

struct SignalFdHandle {
    state : Arc<SignalFdState>,
}

impl VfsIoHandle for SignalFdHandle {
    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        if max_len < SIGNALFD_SIGINFO_SIZE {
            return Err(VfsError::InvalidPath);
        }
        Ok(Box::new(SignalFdPreparedRead { state : self.state.clone(),
                                           max_records : (max_len / SIGNALFD_SIGINFO_SIZE)
                                               .min(ipc::signal::NSIG) }))
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata { node_type : VfsNodeType::Special,
                         size : 0,
                         mode : 0o600,
                         device_major : 0,
                         device_minor : 0,
                         inode : Arc::as_ptr(&self.state) as usize as u64,
                         mount_id : 0,
                         nlink : 1,
                         uid : 0,
                         gid : 0 })
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self { state : self.state.clone() }))
    }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        Ok(if events & POLLIN != 0 && self.state.pending_for(task_id) { POLLIN } else { 0 })
    }

    fn poll_wait_for_ticks(&mut self,
                           events : i16,
                           timeout_ticks : u64,
                           still_waiting : &mut dyn FnMut() -> bool)
                           -> VfsResult<()> {
        let task_id = task::current_task_id().ok_or(VfsError::NoTask)?;
        if events & POLLIN == 0 || self.state.pending_for(task_id) || timeout_ticks == 0 {
            return Ok(());
        }
        ipc::signal::begin_signal_wait(task_id, self.state.mask())
            .map_err(|_| VfsError::NoTask)?;
        let result = self.state.wait.wait_current_while_for_ticks(timeout_ticks, || {
            still_waiting() && !self.state.pending_for(task_id)
        });
        let _ = ipc::signal::end_signal_wait(task_id);
        if result == task::TaskWaitResult::Interrupted && !self.state.pending_for(task_id) {
            Err(VfsError::Interrupted)
        } else {
            Ok(())
        }
    }

    fn open_status_flags(&self) -> u32 {
        if self.state.inner.lock().nonblocking { SFD_NONBLOCK as u32 } else { 0 }
    }

    fn open_accmode(&self) -> u32 { 0 }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        self.state.inner.lock().nonblocking = flags & SFD_NONBLOCK as u32 != 0;
        Ok(())
    }
}

struct ReservedSignal {
    pending : TakenPendingSignal,
    process_pid : usize,
}

struct SignalFdPreparedRead {
    state : Arc<SignalFdState>,
    max_records : usize,
}

impl VfsPreparedRead for SignalFdPreparedRead {
    fn acquire(self : Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let snapshot = task::current_process_task_snapshot().ok_or(VfsError::NoTask)?;
        let task_id = snapshot.task_id;
        let process_pid = snapshot.pid.raw();
        loop {
            let mask = self.state.mask();
            let mut records = Vec::new();
            records.try_reserve_exact(self.max_records).map_err(|_| VfsError::NoMemory)?;
            if let Some(first) = ipc::signal::take_pending_record(task_id, mask) {
                records.push(ReservedSignal { pending : first, process_pid });
                while records.len() < self.max_records {
                    let Some(pending) = ipc::signal::take_pending_record(task_id, mask) else {
                        break;
                    };
                    records.push(ReservedSignal { pending, process_pid });
                }
                return SignalFdReadLease::new(self.state.clone(), task_id, records)
                    .map(|lease| Box::new(lease) as Box<dyn VfsReadLease>);
            }
            if self.state.inner.lock().nonblocking {
                return Err(VfsError::WouldBlock);
            }
            ipc::signal::begin_signal_wait(task_id, mask).map_err(|_| VfsError::NoTask)?;
            let result = self.state.wait.wait_current_while(|| !self.state.pending_for(task_id));
            let _ = ipc::signal::end_signal_wait(task_id);
            if result == task::TaskWaitResult::Interrupted && !self.state.pending_for(task_id) {
                return Err(VfsError::Interrupted);
            }
        }
    }
}

struct SignalFdReadLease {
    state : Arc<SignalFdState>,
    task_id : usize,
    records : Vec<ReservedSignal>,
    bytes : Vec<u8>,
    finished : bool,
}

impl SignalFdReadLease {
    fn new(state : Arc<SignalFdState>,
           task_id : usize,
           records : Vec<ReservedSignal>)
           -> VfsResult<Self> {
        let mut bytes = Vec::new();
        // records 上限为 NSIG，因此乘法严格有界。
        let byte_len = records.len() * SIGNALFD_SIGINFO_SIZE;
        if bytes.try_reserve_exact(byte_len).is_err() {
            for record in &records {
                let _ = ipc::signal::restore_pending_record(task_id, record.pending);
            }
            state.wait.wake_all();
            return Err(VfsError::NoMemory);
        }
        for record in &records {
            let source = peek_pending_signal_source(record.process_pid, record.pending.signal);
            let info = SignalFdSigInfo { signo : record.pending.signal as u32,
                                         pid : source.pid as u32,
                                         uid : source.uid,
                                         ..SignalFdSigInfo::default() };
            let encoded = unsafe {
                core::slice::from_raw_parts(&info as *const SignalFdSigInfo as *const u8,
                                            SIGNALFD_SIGINFO_SIZE)
            };
            bytes.extend_from_slice(encoded);
        }
        Ok(Self { state, task_id, records, bytes, finished : false })
    }

    fn restore_from(&mut self, first : usize) {
        for record in self.records.iter().skip(first) {
            let _ = ipc::signal::restore_pending_record(self.task_id, record.pending);
        }
        if first < self.records.len() {
            self.state.wait.wake_all();
        }
    }
}

impl VfsReadLease for SignalFdReadLease {
    fn bytes(&self) -> &[u8] { &self.bytes }

    fn finish(mut self : Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied > self.bytes.len() {
            return Err(VfsError::Io);
        }
        let committed = if progress.complete {
            self.records.len()
        } else {
            progress.copied / SIGNALFD_SIGINFO_SIZE
        };
        for record in self.records.iter().take(committed) {
            let _ = take_pending_signal_source(record.process_pid, record.pending.signal);
        }
        self.restore_from(committed);
        self.finished = true;
        let copied = committed * SIGNALFD_SIGINFO_SIZE;
        if copied > 0 || progress.complete {
            Ok(VfsReadFinish::Bytes(copied))
        } else {
            Ok(VfsReadFinish::Fault)
        }
    }
}

impl Drop for SignalFdReadLease {
    fn drop(&mut self) {
        if !self.finished {
            self.restore_from(0);
        }
    }
}

pub(crate) fn sys_signalfd4(args : SyscallArgs) -> UserRet {
    let raw_fd = args.arg(0) as isize;
    let mask_ptr = args.arg(1);
    let mask_size = args.arg(2);
    let flags = args.arg(3);
    if mask_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if mask_size != core::mem::size_of::<u64>() ||
       flags & !(SFD_NONBLOCK | SFD_CLOEXEC) != 0
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if super::signal::ensure_current_signal_state().is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    let bits = match copy_from_user_struct::<u64>(mask_ptr) {
        Ok(bits) => bits,
        Err(error) => return UserRet::from_error(error),
    };
    let mut mask = SignalSet::from_bits(bits);
    mask.remove(SIGKILL);
    mask.remove(SIGSTOP);

    if raw_fd >= 0 {
        if flags != 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        return match fd::with_current_io(raw_fd as usize, |handle| {
            let signal_fd = handle.as_any().downcast_ref::<SignalFdHandle>()
                                  .ok_or(VfsError::InvalidPath)?;
            signal_fd.state.update_mask(mask);
            Ok(())
        }) {
            Ok(()) => UserRet::from_success(raw_fd as usize),
            Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
        };
    }

    if raw_fd != -1 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let signal_fd = match fd::alloc_fd(Box::new(SignalFdHandle {
        state : Arc::new(SignalFdState::new(mask, flags & SFD_NONBLOCK != 0)),
    })) {
        Ok(fd) => fd,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if flags & SFD_CLOEXEC != 0 {
        if let Err(error) = fd::set_fd_flags(signal_fd, FD_CLOEXEC) {
            let _ = fd::close_fd(signal_fd);
            return UserRet::from_error(vfs_error_to_errno(error));
        }
    }
    UserRet::from_success(signal_fd)
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    assert_eq!(core::mem::size_of::<SignalFdSigInfo>(), 128);
    assert_eq!(core::mem::offset_of!(SignalFdSigInfo, pid), 12);
    assert_eq!(core::mem::offset_of!(SignalFdSigInfo, ptr), 48);
}
