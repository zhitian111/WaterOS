//! Per-inode POSIX 记录锁与 `flock(2)` 整文件锁状态。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use api_v0::{VfsError, VfsMetadata, VfsNodeType, VfsResult};
use spin::Mutex;
use task::ProcessId;
use waitqueue::{TaskWaitResult, WaitQueue};

/// Linux `F_RDLCK`。
pub const F_RDLCK: i16 = 0;
/// Linux `F_WRLCK`。
pub const F_WRLCK: i16 = 1;
/// Linux `F_UNLCK`。
pub const F_UNLCK: i16 = 2;

/// Linux `struct flock`（64 位 ABI）。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
// 本结构代码由AI完成
pub struct Flock {
    pub l_type: i16,
    pub l_whence: i16,
    pub l_start: i64,
    pub l_len: i64,
    pub l_pid: i32,
}

pub const LOCK_SH: usize = 1;
pub const LOCK_EX: usize = 2;
pub const LOCK_UN: usize = 8;
pub const LOCK_NB: usize = 4;

/// 文件系统内 inode 键（`mount_id` + `inode`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
// 本结构代码由AI完成
pub struct InodeKey {
    pub mount_id: u64,
    pub inode: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LockType {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug)]
// 本结构代码由AI完成
struct PosixLock {
    pid: ProcessId,
    typ: LockType,
    start: u64,
    len: u64,
}

// 本结构代码由AI完成
struct FlockState {
    shared_holders: Vec<u64>,
    exclusive: Option<u64>,
}

impl FlockState {
// 本方法代码由AI完成
    fn new() -> Self {
        Self {
            shared_holders: Vec::new(),
            exclusive: None,
        }
    }

// 本方法代码由AI完成
    fn clear_owner(&mut self, owner: u64) {
        if self.exclusive == Some(owner) {
            self.exclusive = None;
        }
        self.shared_holders.retain(|p| *p != owner);
    }
}

// 本结构代码由AI完成
struct InodeLockData {
    posix: Vec<PosixLock>,
    flock: FlockState,
}

// 本结构代码由AI完成
struct InodeLocks {
    data: Mutex<InodeLockData>,
    wait: WaitQueue,
}

impl InodeLocks {
// 本方法代码由AI完成
    fn new() -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(InodeLockData {
                posix: Vec::new(),
                flock: FlockState::new(),
            }),
            wait: WaitQueue::new_named("file-lock"),
        })
    }
}

// 本变量代码由AI完成
static LOCK_TABLE: Mutex<BTreeMap<InodeKey, Arc<InodeLocks>>> = Mutex::new(BTreeMap::new());

// 本方法代码由AI完成
fn get_inode_locks(key: &InodeKey) -> Arc<InodeLocks> {
    let mut table = LOCK_TABLE.lock();
    table
        .entry(*key)
        .or_insert_with(InodeLocks::new)
        .clone()
}

// 本方法代码由AI完成
fn drop_inode_if_empty(key: &InodeKey, locks: &Arc<InodeLocks>) {
    let data = locks.data.lock();
    if data.posix.is_empty()
        && data.flock.exclusive.is_none()
        && data.flock.shared_holders.is_empty()
    {
        drop(data);
        LOCK_TABLE.lock().remove(key);
    }
}

/// 从句柄元数据构造 inode 键；仅普通文件可上锁。
// 本方法代码由AI完成
pub fn inode_key_from_metadata(meta: &VfsMetadata) -> Option<InodeKey> {
    if meta.node_type != VfsNodeType::File {
        return None;
    }
    Some(InodeKey {
        mount_id: meta.mount_id,
        inode: meta.inode,
    })
}

// 本方法代码由AI完成
fn lock_end(start: u64, len: u64) -> u64 {
    if len == 0 {
        u64::MAX
    } else {
        start.saturating_add(len.saturating_sub(1))
    }
}

// 本方法代码由AI完成
fn ranges_overlap(a_start: u64, a_len: u64, b_start: u64, b_len: u64) -> bool {
    let a_end = lock_end(a_start, a_len);
    let b_end = lock_end(b_start, b_len);
    a_start <= b_end && b_start <= a_end
}

// 本方法代码由AI完成
fn flock_type_from_i16(raw: i16) -> VfsResult<LockType> {
    match raw {
        F_RDLCK => Ok(LockType::Read),
        F_WRLCK => Ok(LockType::Write),
        _ => Err(VfsError::Unsupported),
    }
}

// 本方法代码由AI完成
fn intersection_range(
    probe_start: u64,
    probe_len: u64,
    lock_start: u64,
    lock_len: u64,
) -> Option<(u64, u64)> {
    let probe_end = lock_end(probe_start, probe_len);
    let lock_end_pos = lock_end(lock_start, lock_len);
    let start = probe_start.max(lock_start);
    if start > probe_end || start > lock_end_pos {
        return None;
    }
    let end = probe_end.min(lock_end_pos);
    if end == u64::MAX {
        return Some((start, 0));
    }
    Some((start, end.saturating_sub(start).saturating_add(1)))
}

// 本方法代码由AI完成
fn getlk_conflicts_with_probe(probe_typ: LockType, held: LockType) -> bool {
    match probe_typ {
        LockType::Read => held == LockType::Write,
        LockType::Write => true,
    }
}

/// 在 `locks` 中找与 probe 冲突、且起始字节最靠前的锁，并返回与 probe 的交集区间。
// 本方法代码由AI完成
fn find_getlk_conflict(
    locks: &[PosixLock],
    pid: ProcessId,
    probe_typ: LockType,
    probe_start: u64,
    probe_len: u64,
) -> Option<(PosixLock, u64, u64)> {
    let mut best: Option<(PosixLock, u64, u64)> = None;
    for lock in locks {
        if lock.pid == pid || !getlk_conflicts_with_probe(probe_typ, lock.typ) {
            continue;
        }
        let Some((isect_start, isect_len)) =
            intersection_range(probe_start, probe_len, lock.start, lock.len)
        else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, best_start, _)| isect_start < *best_start) {
            best = Some((*lock, isect_start, isect_len));
        }
    }
    best
}

// 本方法代码由AI完成
fn posix_conflict(
    locks: &[PosixLock],
    pid: ProcessId,
    typ: LockType,
    start: u64,
    len: u64,
) -> Option<PosixLock> {
    for lock in locks {
        if lock.pid == pid {
            continue;
        }
        if !ranges_overlap(lock.start, lock.len, start, len) {
            continue;
        }
        match (typ, lock.typ) {
            (LockType::Read, LockType::Read) => {}
            _ => return Some(*lock),
        }
    }
    None
}

// 本方法代码由AI完成
fn flock_blocks_posix(flock: &FlockState, typ: LockType) -> bool {
    match typ {
        LockType::Read => flock.exclusive.is_some(),
        LockType::Write => flock.exclusive.is_some() || !flock.shared_holders.is_empty(),
    }
}

// 本方法代码由AI完成
fn posix_blocks_flock(locks: &[PosixLock], pid: ProcessId, op: usize) -> bool {
    if (op & LOCK_EX) != 0 {
        return locks.iter().any(|l| l.pid != pid);
    }
    if (op & LOCK_SH) != 0 {
        return locks
            .iter()
            .any(|l| l.pid != pid && l.typ == LockType::Write);
    }
    false
}

// 本方法代码由AI完成
fn split_lock_around_region(lock: PosixLock, cut_start: u64, cut_len: u64) -> Vec<PosixLock> {
    let cut_end = lock_end(cut_start, cut_len);
    let lock_end_pos = lock_end(lock.start, lock.len);
    if cut_start > lock_end_pos || cut_end < lock.start {
        return vec![lock];
    }
    let mut out = Vec::new();
    if lock.start < cut_start {
        out.push(PosixLock {
            pid: lock.pid,
            typ: lock.typ,
            start: lock.start,
            len: cut_start - lock.start,
        });
    }
    if cut_end < lock_end_pos {
        let right_start = cut_end.saturating_add(1);
        if lock.len == 0 {
            out.push(PosixLock {
                pid: lock.pid,
                typ: lock.typ,
                start: right_start,
                len: 0,
            });
        } else {
            out.push(PosixLock {
                pid: lock.pid,
                typ: lock.typ,
                start: right_start,
                len: lock_end_pos - right_start + 1,
            });
        }
    }
    out
}

// 本方法代码由AI完成
fn remove_posix_for_pid_region(
    locks: &mut Vec<PosixLock>,
    pid: ProcessId,
    start: u64,
    len: u64,
) {
    let mut kept = Vec::with_capacity(locks.len());
    for lock in locks.drain(..) {
        if lock.pid != pid || !ranges_overlap(lock.start, lock.len, start, len) {
            kept.push(lock);
        } else {
            kept.extend(split_lock_around_region(lock, start, len));
        }
    }
    *locks = kept;
}

// 本方法代码由AI完成
fn apply_posix_unlock(locks: &mut Vec<PosixLock>, pid: ProcessId, start: u64, len: u64) {
    remove_posix_for_pid_region(locks, pid, start, len);
}

// 本方法代码由AI完成
fn apply_posix_lock(
    locks: &mut Vec<PosixLock>,
    pid: ProcessId,
    typ: LockType,
    start: u64,
    len: u64,
) {
    remove_posix_for_pid_region(locks, pid, start, len);
    locks.push(PosixLock {
        pid,
        typ,
        start,
        len,
    });
}

// 本方法代码由AI完成
fn posix_would_block(data: &InodeLockData, pid: ProcessId, typ: LockType, start: u64, len: u64) -> bool {
    flock_blocks_posix(&data.flock, typ)
        || posix_conflict(&data.posix, pid, typ, start, len).is_some()
}

/// `fcntl(F_GETLK)`：无冲突时写回 `F_UNLCK`；有冲突时写回阻塞方信息。
// 本方法代码由AI完成
pub fn posix_getlk(
    key: &InodeKey,
    pid: ProcessId,
    flock: &mut Flock,
) -> VfsResult<()> {
    let typ = flock_type_from_i16(flock.l_type)?;
    let start = flock.l_start as u64;
    let len = flock.l_len as u64;
    let locks = get_inode_locks(key);
    let data = locks.data.lock();

    if flock_blocks_posix(&data.flock, typ) {
        flock.l_type = if data.flock.exclusive.is_some() {
            F_WRLCK
        } else {
            F_RDLCK
        };
        flock.l_whence = 0;
        flock.l_start = 0;
        flock.l_len = 0;
        flock.l_pid = data
            .flock
            .exclusive
            .or_else(|| data.flock.shared_holders.first().copied())
            .map(|p| p as i32)
            .unwrap_or(0);
        return Ok(());
    }

    if let Some((conflict, isect_start, isect_len)) =
        find_getlk_conflict(&data.posix, pid, typ, start, len)
    {
        flock.l_type = match conflict.typ {
            LockType::Read => F_RDLCK,
            LockType::Write => F_WRLCK,
        };
        flock.l_start = isect_start as i64;
        flock.l_len = isect_len as i64;
        flock.l_pid = conflict.pid.0 as i32;
        return Ok(());
    }

    // Linux：无冲突时仅清除 l_type，其余字段（含 l_pid）保持不变（fcntl05）。
    flock.l_type = F_UNLCK;
    Ok(())
}

/// `fcntl(F_SETLK)` / `F_SETLKW`。
// 本方法代码由AI完成
pub fn posix_setlk(
    key: &InodeKey,
    pid: ProcessId,
    flock: &Flock,
    blocking: bool,
) -> VfsResult<()> {
    let start = flock.l_start as u64;
    let len = flock.l_len as u64;
    let locks = get_inode_locks(key);

    if flock.l_type == F_UNLCK {
        {
            let mut data = locks.data.lock();
            apply_posix_unlock(&mut data.posix, pid, start, len);
        }
        locks.wait.wake_all();
        drop_inode_if_empty(key, &locks);
        return Ok(());
    }

    let typ = flock_type_from_i16(flock.l_type)?;

    loop {
        let blocked = {
            let mut data = locks.data.lock();
            if posix_would_block(&data, pid, typ, start, len) {
                true
            } else {
                apply_posix_lock(&mut data.posix, pid, typ, start, len);
                false
            }
        };

        if !blocked {
            locks.wait.wake_all();
            return Ok(());
        }
        if !blocking {
            return Err(VfsError::WouldBlock);
        }

        let wait_result = locks.wait.wait_current_while(|| {
            let data = locks.data.lock();
            posix_would_block(&data, pid, typ, start, len)
        });
        if wait_result == TaskWaitResult::Interrupted {
            return Err(VfsError::Interrupted);
        }
    }
}

/// `flock(2)`。
// 本方法代码由AI完成
pub fn flock_op(key: &InodeKey, pid: ProcessId, owner: u64, operation: usize) -> VfsResult<()> {
    let nonblocking = (operation & LOCK_NB) != 0;
    let op = operation & !LOCK_NB;

    if op != LOCK_SH && op != LOCK_EX && op != LOCK_UN {
        return Err(VfsError::Unsupported);
    }

    let locks = get_inode_locks(key);

    loop {
        let blocked = {
            let mut data = locks.data.lock();
            match op {
                LOCK_UN => {
                    data.flock.clear_owner(owner);
                    false
                }
                LOCK_SH => {
                    if data.flock.exclusive.is_some() && data.flock.exclusive != Some(owner) {
                        true
                    } else if posix_blocks_flock(&data.posix, pid, LOCK_SH) {
                        true
                    } else {
                        if !data.flock.shared_holders.iter().any(|p| *p == owner) {
                            data.flock.shared_holders.push(owner);
                        }
                        data.flock.exclusive = None;
                        false
                    }
                }
                LOCK_EX => {
                    if data.flock.exclusive.is_some() && data.flock.exclusive != Some(owner) {
                        true
                    } else if !data.flock.shared_holders.is_empty()
                        && !(data.flock.shared_holders.len() == 1
                            && data.flock.shared_holders[0] == owner)
                    {
                        true
                    } else if posix_blocks_flock(&data.posix, pid, LOCK_EX) {
                        true
                    } else {
                        data.flock.shared_holders.clear();
                        data.flock.exclusive = Some(owner);
                        false
                    }
                }
                _ => return Err(VfsError::Unsupported),
            }
        };

        if !blocked {
            locks.wait.wake_all();
            drop_inode_if_empty(key, &locks);
            return Ok(());
        }
        if nonblocking {
            return Err(VfsError::WouldBlock);
        }

        let wait_result = locks.wait.wait_current_while(|| {
            let data = locks.data.lock();
            match op {
                LOCK_SH => {
                    data.flock.exclusive.is_some() && data.flock.exclusive != Some(owner)
                        || posix_blocks_flock(&data.posix, pid, LOCK_SH)
                }
                LOCK_EX => {
                    (data.flock.exclusive.is_some() && data.flock.exclusive != Some(owner))
                        || (!data.flock.shared_holders.is_empty()
                            && !(data.flock.shared_holders.len() == 1
                                && data.flock.shared_holders[0] == owner))
                        || posix_blocks_flock(&data.posix, pid, LOCK_EX)
                }
                _ => false,
            }
        });
        if wait_result == TaskWaitResult::Interrupted {
            return Err(VfsError::Interrupted);
        }
    }
}

/// 关闭进程在 inode 上的全部锁（POSIX + flock）。
// 本方法代码由AI完成
pub fn release_process_inode_locks(pid: ProcessId, key: &InodeKey) {
    let Some(locks) = LOCK_TABLE.lock().get(key).cloned() else {
        return;
    };
    {
        let mut data = locks.data.lock();
        data.posix.retain(|lock| lock.pid != pid);
    }
    locks.wait.wake_all();
    drop_inode_if_empty(key, &locks);
}

/// 关闭一个打开文件描述时释放其 `flock(2)` 锁。
// 本方法代码由AI完成
pub fn release_flock_owner(key: &InodeKey, owner: u64) {
    let Some(locks) = LOCK_TABLE.lock().get(key).cloned() else {
        return;
    };
    {
        let mut data = locks.data.lock();
        data.flock.clear_owner(owner);
    }
    locks.wait.wake_all();
    drop_inode_if_empty(key, &locks);
}
