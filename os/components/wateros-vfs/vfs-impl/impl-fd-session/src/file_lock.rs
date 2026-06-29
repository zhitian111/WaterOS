//! Per-inode POSIX 记录锁与 `flock(2)` 整文件锁状态。

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
struct PosixLock {
    pid: ProcessId,
    typ: LockType,
    start: u64,
    len: u64,
}

struct FlockState {
    shared_holders: Vec<ProcessId>,
    exclusive: Option<ProcessId>,
}

impl FlockState {
    fn new() -> Self {
        Self {
            shared_holders: Vec::new(),
            exclusive: None,
        }
    }

    fn clear_pid(&mut self, pid: ProcessId) {
        if self.exclusive == Some(pid) {
            self.exclusive = None;
        }
        self.shared_holders.retain(|p| *p != pid);
    }
}

struct InodeLockData {
    posix: Vec<PosixLock>,
    flock: FlockState,
}

struct InodeLocks {
    data: Mutex<InodeLockData>,
    wait: WaitQueue,
}

impl InodeLocks {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(InodeLockData {
                posix: Vec::new(),
                flock: FlockState::new(),
            }),
            wait: WaitQueue::new(),
        })
    }
}

static LOCK_TABLE: Mutex<BTreeMap<InodeKey, Arc<InodeLocks>>> = Mutex::new(BTreeMap::new());

fn get_inode_locks(key: &InodeKey) -> Arc<InodeLocks> {
    let mut table = LOCK_TABLE.lock();
    table
        .entry(*key)
        .or_insert_with(InodeLocks::new)
        .clone()
}

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
#[inline]
pub fn inode_key_from_metadata(meta: &VfsMetadata) -> Option<InodeKey> {
    if meta.node_type != VfsNodeType::File {
        return None;
    }
    Some(InodeKey {
        mount_id: meta.mount_id,
        inode: meta.inode,
    })
}

fn lock_end(start: u64, len: u64) -> u64 {
    if len == 0 {
        u64::MAX
    } else {
        start.saturating_add(len.saturating_sub(1))
    }
}

fn ranges_overlap(a_start: u64, a_len: u64, b_start: u64, b_len: u64) -> bool {
    let a_end = lock_end(a_start, a_len);
    let b_end = lock_end(b_start, b_len);
    a_start <= b_end && b_start <= a_end
}

fn flock_type_from_i16(raw: i16) -> VfsResult<LockType> {
    match raw {
        F_RDLCK => Ok(LockType::Read),
        F_WRLCK => Ok(LockType::Write),
        _ => Err(VfsError::Unsupported),
    }
}

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

fn getlk_conflicts_with_probe(probe_typ: LockType, held: LockType) -> bool {
    match probe_typ {
        LockType::Read => held == LockType::Write,
        LockType::Write => true,
    }
}

/// 在 `locks` 中找与 probe 冲突、且起始字节最靠前的锁，并返回与 probe 的交集区间。
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

fn flock_blocks_posix(flock: &FlockState, typ: LockType) -> bool {
    match typ {
        LockType::Read => flock.exclusive.is_some(),
        LockType::Write => flock.exclusive.is_some() || !flock.shared_holders.is_empty(),
    }
}

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

fn apply_posix_unlock(locks: &mut Vec<PosixLock>, pid: ProcessId, start: u64, len: u64) {
    remove_posix_for_pid_region(locks, pid, start, len);
}

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

fn posix_would_block(data: &InodeLockData, pid: ProcessId, typ: LockType, start: u64, len: u64) -> bool {
    flock_blocks_posix(&data.flock, typ)
        || posix_conflict(&data.posix, pid, typ, start, len).is_some()
}

/// `fcntl(F_GETLK)`：无冲突时写回 `F_UNLCK`；有冲突时写回阻塞方信息。
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
            .map(|p| p.0 as i32)
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
pub fn flock_op(key: &InodeKey, pid: ProcessId, operation: usize) -> VfsResult<()> {
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
                    data.flock.clear_pid(pid);
                    false
                }
                LOCK_SH => {
                    if data.flock.exclusive.is_some() && data.flock.exclusive != Some(pid) {
                        true
                    } else if posix_blocks_flock(&data.posix, pid, LOCK_SH) {
                        true
                    } else {
                        if !data.flock.shared_holders.iter().any(|p| *p == pid) {
                            data.flock.shared_holders.push(pid);
                        }
                        data.flock.exclusive = None;
                        false
                    }
                }
                LOCK_EX => {
                    if data.flock.exclusive.is_some() && data.flock.exclusive != Some(pid) {
                        true
                    } else if !data.flock.shared_holders.is_empty()
                        && !(data.flock.shared_holders.len() == 1
                            && data.flock.shared_holders[0] == pid)
                    {
                        true
                    } else if posix_blocks_flock(&data.posix, pid, LOCK_EX) {
                        true
                    } else {
                        data.flock.shared_holders.clear();
                        data.flock.exclusive = Some(pid);
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
                    data.flock.exclusive.is_some() && data.flock.exclusive != Some(pid)
                        || posix_blocks_flock(&data.posix, pid, LOCK_SH)
                }
                LOCK_EX => {
                    (data.flock.exclusive.is_some() && data.flock.exclusive != Some(pid))
                        || (!data.flock.shared_holders.is_empty()
                            && !(data.flock.shared_holders.len() == 1
                                && data.flock.shared_holders[0] == pid))
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
pub fn release_process_inode_locks(pid: ProcessId, key: &InodeKey) {
    let Some(locks) = LOCK_TABLE.lock().get(key).cloned() else {
        return;
    };
    {
        let mut data = locks.data.lock();
        data.posix.retain(|lock| lock.pid != pid);
        data.flock.clear_pid(pid);
    }
    locks.wait.wake_all();
    drop_inode_if_empty(key, &locks);
}

/// `fork` 后子进程继承父进程持有的全部记录锁与 flock 状态。
pub fn inherit_process_locks(parent_pid: ProcessId, child_pid: ProcessId) {
    let table = LOCK_TABLE.lock();
    for locks in table.values() {
        let mut data = locks.data.lock();
        let parent_posix: Vec<PosixLock> = data
            .posix
            .iter()
            .filter(|lock| lock.pid == parent_pid)
            .copied()
            .collect();
        for lock in parent_posix {
            data.posix.push(PosixLock {
                pid: child_pid,
                typ: lock.typ,
                start: lock.start,
                len: lock.len,
            });
        }
    }
}
