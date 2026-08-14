//! SysV 信号量组。
//!
//! 一次 `semop` 的整组操作在同一把短锁内先模拟、后提交，因此不会留下
//! 部分更新。阻塞时释放 registry 锁并按 tick 休眠；`IPC_RMID` 后等待者返回
//! `EIDRM`。`SEM_UNDO` 以 task 为生命周期单位，在任务资源清理时回放。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use api_v0::{ErrNo, SyscallArgs, UserRet};
use spin::Mutex;

use crate::poll_engine::PollDeadline;
use crate::user_copy::{copy_from_user_struct, copy_to_user, copy_to_user_struct};

const IPC_PRIVATE : i32 = 0;
const IPC_CREAT : usize = 0o1000;
const IPC_EXCL : usize = 0o2000;
const IPC_NOWAIT : i16 = 0o4000;
const SEM_UNDO : i16 = 0o10000;
const IPC_RMID : usize = 0;
const IPC_SET : usize = 1;
const IPC_STAT : usize = 2;
const IPC_64 : usize = 0x100;

const GETPID : usize = 11;
const GETVAL : usize = 12;
const GETALL : usize = 13;
const GETNCNT : usize = 14;
const GETZCNT : usize = 15;
const SETVAL : usize = 16;
const SETALL : usize = 17;

const SEMMSL : usize = 256;
const SEMOPM : usize = 500;
const SEMVMX : i32 = 32_767;
const SEMAEM : i32 = SEMVMX;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSemBuf {
    num : u16,
    op : i16,
    flags : i16,
}

const _ : () = assert!(core::mem::size_of::<UserSemBuf>() == 6);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Ipc64Perm {
    key : i32,
    uid : u32,
    gid : u32,
    cuid : u32,
    cgid : u32,
    mode : u32,
    pad1 : u32,
    seq : u16,
    pad2 : u16,
    unused1 : u64,
    unused2 : u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Semid64Ds {
    perm : Ipc64Perm,
    otime : i64,
    ctime : i64,
    nsems : u64,
    unused3 : u64,
    unused4 : u64,
}

const _ : () = assert!(core::mem::size_of::<Ipc64Perm>() == 48);
const _ : () = assert!(core::mem::size_of::<Semid64Ds>() == 88);

struct SemaphoreSet {
    key : i32,
    uid : u32,
    gid : u32,
    cuid : u32,
    cgid : u32,
    mode : u32,
    values : Vec<i32>,
    last_pid : Vec<i32>,
    wait_nonzero : Vec<u32>,
    wait_zero : Vec<u32>,
    otime : i64,
    ctime : i64,
}

struct SemaphoreRegistry {
    next_id : i32,
    by_id : BTreeMap<i32, SemaphoreSet>,
    by_key : BTreeMap<i32, i32>,
    // 应在 task 退出时加回 semval 的调整量。
    undo : BTreeMap<(usize, i32, u16), i32>,
}

impl SemaphoreRegistry {
    const fn new() -> Self {
        Self { next_id : 1,
               by_id : BTreeMap::new(),
               by_key : BTreeMap::new(),
               undo : BTreeMap::new() }
    }

    fn alloc_id(&mut self) -> Result<i32, ErrNo> {
        let start = self.next_id.max(1);
        loop {
            let id = self.next_id.max(1);
            self.next_id = self.next_id.checked_add(1).unwrap_or(1);
            if !self.by_id.contains_key(&id) {
                return Ok(id);
            }
            if self.next_id == start {
                return Err(ErrNo::ENOSPC);
            }
        }
    }
}

static SEMAPHORES : Mutex<SemaphoreRegistry> = Mutex::new(SemaphoreRegistry::new());

fn identity() -> (u32, u32) {
    let credentials = cred::current_credentials();
    (credentials.effective_uid.0, credentials.effective_gid.0)
}

fn process_id() -> i32 {
    task::current_process_snapshot()
        .map(|process| process.pid.raw().min(i32::MAX as usize) as i32)
        .unwrap_or(0)
}

fn task_id() -> Result<usize, ErrNo> { task::current_task_id().ok_or(ErrNo::ESRCH) }

fn now_seconds() -> i64 {
    platform::wall_clock::realtime_ns()
        .map(|ns| (ns / 1_000_000_000).min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn has_access(set : &SemaphoreSet, alter : bool, uid : u32, gid : u32) -> bool {
    if uid == 0 {
        return true;
    }
    let shift = if uid == set.uid || uid == set.cuid {
        6
    } else if gid == set.gid || gid == set.cgid {
        3
    } else {
        0
    };
    let required = if alter { 0o2 } else { 0o4 };
    ((set.mode >> shift) & required) != 0
}

pub(crate) fn sys_semget(args : SyscallArgs) -> UserRet {
    let key = args.arg(0) as i32;
    let nsems = args.arg(1);
    let flags = args.arg(2);
    let (uid, gid) = identity();
    let timestamp = now_seconds();
    let result = {
        let mut registry = SEMAPHORES.lock();
        if key != IPC_PRIVATE {
            if let Some(id) = registry.by_key.get(&key).copied() {
                if flags & IPC_CREAT != 0 && flags & IPC_EXCL != 0 {
                    Err(ErrNo::EEXIST)
                } else {
                    match registry.by_id.get(&id) {
                        Some(set) if nsems <= set.values.len() => {
                            let requested = flags & 0o666;
                            let read_ok = requested & 0o444 == 0 ||
                                          has_access(set, false, uid, gid);
                            let write_ok = requested & 0o222 == 0 ||
                                           has_access(set, true, uid, gid);
                            if read_ok && write_ok { Ok(id) } else { Err(ErrNo::EACCES) }
                        }
                        Some(_) | None => Err(ErrNo::EINVAL),
                    }
                }
            } else if flags & IPC_CREAT == 0 {
                Err(ErrNo::ENOENT)
            } else {
                create_set(&mut registry, key, nsems, flags, uid, gid, timestamp)
            }
        } else {
            create_set(&mut registry, key, nsems, flags, uid, gid, timestamp)
        }
    };
    match result {
        Ok(id) => UserRet::from_success(id as usize),
        Err(error) => UserRet::from_error(error),
    }
}

fn create_set(registry : &mut SemaphoreRegistry,
              key : i32,
              nsems : usize,
              flags : usize,
              uid : u32,
              gid : u32,
              timestamp : i64)
              -> Result<i32, ErrNo> {
    if nsems == 0 || nsems > SEMMSL {
        return Err(ErrNo::EINVAL);
    }
    let id = registry.alloc_id()?;
    let mut values = Vec::new();
    values.try_reserve_exact(nsems).map_err(|_| ErrNo::ENOMEM)?;
    values.resize(nsems, 0);
    let mut last_pid = Vec::new();
    last_pid.try_reserve_exact(nsems).map_err(|_| ErrNo::ENOMEM)?;
    last_pid.resize(nsems, 0);
    let mut wait_nonzero = Vec::new();
    wait_nonzero.try_reserve_exact(nsems).map_err(|_| ErrNo::ENOMEM)?;
    wait_nonzero.resize(nsems, 0);
    let mut wait_zero = Vec::new();
    wait_zero.try_reserve_exact(nsems).map_err(|_| ErrNo::ENOMEM)?;
    wait_zero.resize(nsems, 0);
    registry.by_id.insert(id,
                          SemaphoreSet { key,
                                         uid,
                                         gid,
                                         cuid : uid,
                                         cgid : gid,
                                         mode : (flags & 0o777) as u32,
                                         values,
                                         last_pid,
                                         wait_nonzero,
                                         wait_zero,
                                         otime : 0,
                                         ctime : timestamp });
    if key != IPC_PRIVATE {
        registry.by_key.insert(key, id);
    }
    Ok(id)
}

fn import_ops(pointer : usize, count : usize) -> Result<Vec<UserSemBuf>, ErrNo> {
    if count == 0 || count > SEMOPM {
        return Err(if count == 0 { ErrNo::EINVAL } else { ErrNo::E2BIG });
    }
    if pointer == 0 {
        return Err(ErrNo::EFAULT);
    }
    let mut operations = Vec::new();
    operations.try_reserve_exact(count).map_err(|_| ErrNo::ENOMEM)?;
    for index in 0..count {
        let address = pointer.checked_add(index.checked_mul(core::mem::size_of::<UserSemBuf>())
                                               .ok_or(ErrNo::EFAULT)?)
                             .ok_or(ErrNo::EFAULT)?;
        let operation = copy_from_user_struct::<UserSemBuf>(address)?;
        if operation.flags & !(IPC_NOWAIT | SEM_UNDO) != 0 {
            return Err(ErrNo::EINVAL);
        }
        operations.push(operation);
    }
    Ok(operations)
}

#[derive(Clone, Copy)]
enum BlockedOn {
    NonZero { index : usize, nowait : bool },
    Zero { index : usize, nowait : bool },
}

fn do_semop(args : SyscallArgs, timed : bool) -> UserRet {
    let id = args.arg(0) as i32;
    let operations = match import_ops(args.arg(1), args.arg(2)) {
        Ok(operations) => operations,
        Err(error) => return UserRet::from_error(error),
    };
    let deadline = if timed {
        match PollDeadline::from_timespec_ptr(args.arg(3)) {
            Ok(deadline) => Some(deadline),
            Err(error) => return UserRet::from_error(error),
        }
    } else {
        None
    };
    let task_id = match task_id() {
        Ok(task_id) => task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let pid = process_id();
    let (uid, gid) = identity();
    let mut observed = false;

    loop {
        let timestamp = now_seconds();
        let outcome = {
            let mut registry = SEMAPHORES.lock();
            let Some(set) = registry.by_id.get(&id) else {
                return UserRet::from_error(if observed { ErrNo::EIDRM } else { ErrNo::EINVAL });
            };
            observed = true;
            if !has_access(set, true, uid, gid) {
                return UserRet::from_error(ErrNo::EACCES);
            }
            let mut scratch = [0i32; SEMMSL];
            scratch[..set.values.len()].copy_from_slice(&set.values);
            let mut blocked = None;
            for operation in &operations {
                let index = operation.num as usize;
                if index >= set.values.len() {
                    return UserRet::from_error(ErrNo::EFBIG);
                }
                if operation.op > 0 {
                    let next = match scratch[index].checked_add(operation.op as i32) {
                        Some(next) if next <= SEMVMX => next,
                        _ => return UserRet::from_error(ErrNo::ERANGE),
                    };
                    scratch[index] = next;
                } else if operation.op < 0 {
                    let need = -(operation.op as i32);
                    if scratch[index] < need {
                        blocked = Some(BlockedOn::NonZero { index,
                                                           nowait : operation.flags &
                                                                    IPC_NOWAIT !=
                                                                    0 });
                        break;
                    }
                    scratch[index] -= need;
                } else if scratch[index] != 0 {
                    blocked = Some(BlockedOn::Zero { index,
                                                     nowait : operation.flags & IPC_NOWAIT != 0 });
                    break;
                }
            }
            if let Some(blocked) = blocked {
                let nowait = match blocked {
                    BlockedOn::NonZero { nowait, .. } | BlockedOn::Zero { nowait, .. } => nowait,
                };
                if nowait {
                    return UserRet::from_error(ErrNo::EAGAIN);
                }
                let set = registry.by_id.get_mut(&id).expect("set checked above");
                match blocked {
                    BlockedOn::NonZero { index, .. } => {
                        set.wait_nonzero[index] = set.wait_nonzero[index].saturating_add(1);
                    }
                    BlockedOn::Zero { index, .. } => {
                        set.wait_zero[index] = set.wait_zero[index].saturating_add(1);
                    }
                }
                Err(blocked)
            } else {
                // SEM_UNDO 也是事务的一部分：先聚合同一 semnum 的调整并验证，
                // 所有检查通过后才修改 semval 和 undo registry。
                let mut undo_delta = [0i32; SEMMSL];
                for operation in &operations {
                    if operation.flags & SEM_UNDO != 0 && operation.op != 0 {
                        let index = operation.num as usize;
                        undo_delta[index] = match undo_delta[index]
                            .checked_sub(operation.op as i32)
                        {
                            Some(value) if value.abs() <= SEMAEM => value,
                            _ => return UserRet::from_error(ErrNo::ERANGE),
                        };
                    }
                }
                for (number, delta) in undo_delta.iter().copied().enumerate() {
                    if delta == 0 {
                        continue;
                    }
                    let current = registry.undo.get(&(task_id, id, number as u16))
                                               .copied()
                                               .unwrap_or(0);
                    match current.checked_add(delta) {
                        Some(value) if value.abs() <= SEMAEM => {}
                        _ => return UserRet::from_error(ErrNo::ERANGE),
                    }
                }
                let set = registry.by_id.get_mut(&id).expect("set checked above");
                let value_count = set.values.len();
                set.values.copy_from_slice(&scratch[..value_count]);
                for operation in &operations {
                    set.last_pid[operation.num as usize] = pid;
                }
                set.otime = timestamp;
                for (number, delta) in undo_delta.iter().copied().enumerate() {
                    if delta == 0 {
                        continue;
                    }
                    let key = (task_id, id, number as u16);
                    let next = registry.undo.get(&key).copied().unwrap_or(0) + delta;
                    if next == 0 {
                        registry.undo.remove(&key);
                    } else {
                        registry.undo.insert(key, next);
                    }
                }
                Ok(())
            }
        };

        let blocked = match outcome {
            Ok(()) => return UserRet::from_success(0),
            Err(blocked) => blocked,
        };
        if deadline.as_ref().is_some_and(PollDeadline::expired) {
            decrement_waiter(id, blocked);
            return UserRet::from_error(ErrNo::EAGAIN);
        }
        let wait_result = task::sleep_for_ticks(1);
        decrement_waiter(id, blocked);
        if wait_result == task::TaskWaitResult::Interrupted {
            return UserRet::from_error(ErrNo::EINTR);
        }
    }
}

fn decrement_waiter(id : i32, blocked : BlockedOn) {
    let mut registry = SEMAPHORES.lock();
    let Some(set) = registry.by_id.get_mut(&id) else { return };
    match blocked {
        BlockedOn::NonZero { index, .. } if index < set.wait_nonzero.len() => {
            set.wait_nonzero[index] = set.wait_nonzero[index].saturating_sub(1);
        }
        BlockedOn::Zero { index, .. } if index < set.wait_zero.len() => {
            set.wait_zero[index] = set.wait_zero[index].saturating_sub(1);
        }
        _ => {}
    }
}

pub(crate) fn sys_semop(args : SyscallArgs) -> UserRet { do_semop(args, false) }

pub(crate) fn sys_semtimedop(args : SyscallArgs) -> UserRet { do_semop(args, true) }

pub(crate) fn sys_semctl(args : SyscallArgs) -> UserRet {
    let id = args.arg(0) as i32;
    let semnum = args.arg(1);
    let command = args.arg(2) & !IPC_64;
    let argument = args.arg(3);
    let (uid, gid) = identity();
    match command {
        IPC_RMID => {
            let mut registry = SEMAPHORES.lock();
            let Some(set) = registry.by_id.get(&id) else {
                return UserRet::from_error(ErrNo::EINVAL);
            };
            if uid != 0 && uid != set.uid && uid != set.cuid {
                return UserRet::from_error(ErrNo::EPERM);
            }
            let set = registry.by_id.remove(&id).expect("set checked above");
            if set.key != IPC_PRIVATE {
                registry.by_key.remove(&set.key);
            }
            registry.undo.retain(|(_, semid, _), _| *semid != id);
            UserRet::from_success(0)
        }
        IPC_STAT => {
            if argument == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let snapshot = {
                let registry = SEMAPHORES.lock();
                let set = match registry.by_id.get(&id) {
                    Some(set) => set,
                    None => return UserRet::from_error(ErrNo::EINVAL),
                };
                if !has_access(set, false, uid, gid) {
                    return UserRet::from_error(ErrNo::EACCES);
                }
                set_snapshot(set)
            };
            match copy_to_user_struct(argument, &snapshot) {
                Ok(()) => UserRet::from_success(0),
                Err(error) => UserRet::from_error(error),
            }
        }
        IPC_SET => {
            let update = match copy_from_user_struct::<Semid64Ds>(argument) {
                Ok(update) => update,
                Err(error) => return UserRet::from_error(error),
            };
            let timestamp = now_seconds();
            let mut registry = SEMAPHORES.lock();
            let set = match registry.by_id.get_mut(&id) {
                Some(set) => set,
                None => return UserRet::from_error(ErrNo::EINVAL),
            };
            if uid != 0 && uid != set.uid && uid != set.cuid {
                return UserRet::from_error(ErrNo::EPERM);
            }
            set.uid = update.perm.uid;
            set.gid = update.perm.gid;
            set.mode = (set.mode & !0o777) | (update.perm.mode & 0o777);
            set.ctime = timestamp;
            UserRet::from_success(0)
        }
        GETVAL | GETPID | GETNCNT | GETZCNT => {
            let registry = SEMAPHORES.lock();
            let set = match registry.by_id.get(&id) {
                Some(set) => set,
                None => return UserRet::from_error(ErrNo::EINVAL),
            };
            if !has_access(set, false, uid, gid) {
                return UserRet::from_error(ErrNo::EACCES);
            }
            if semnum >= set.values.len() {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            let value = match command {
                GETVAL => set.values[semnum] as usize,
                GETPID => set.last_pid[semnum].max(0) as usize,
                GETNCNT => set.wait_nonzero[semnum] as usize,
                GETZCNT => set.wait_zero[semnum] as usize,
                _ => unreachable!(),
            };
            UserRet::from_success(value)
        }
        SETVAL => {
            if argument > SEMVMX as usize {
                return UserRet::from_error(ErrNo::ERANGE);
            }
            let pid = process_id();
            let timestamp = now_seconds();
            let mut registry = SEMAPHORES.lock();
            let set = match registry.by_id.get_mut(&id) {
                Some(set) => set,
                None => return UserRet::from_error(ErrNo::EINVAL),
            };
            if !has_access(set, true, uid, gid) {
                return UserRet::from_error(ErrNo::EACCES);
            }
            if semnum >= set.values.len() {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            set.values[semnum] = argument as i32;
            set.last_pid[semnum] = pid;
            set.ctime = timestamp;
            registry.undo.retain(|(_, semid, number), _| {
                *semid != id || *number as usize != semnum
            });
            UserRet::from_success(0)
        }
        GETALL => {
            if argument == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let values = {
                let registry = SEMAPHORES.lock();
                let set = match registry.by_id.get(&id) {
                    Some(set) => set,
                    None => return UserRet::from_error(ErrNo::EINVAL),
                };
                if !has_access(set, false, uid, gid) {
                    return UserRet::from_error(ErrNo::EACCES);
                }
                set.values.iter().map(|value| *value as u16).collect::<Vec<_>>()
            };
            let bytes = unsafe {
                core::slice::from_raw_parts(values.as_ptr().cast::<u8>(),
                                            values.len() * core::mem::size_of::<u16>())
            };
            match copy_to_user(argument, bytes) {
                Ok(copied) if copied == bytes.len() => UserRet::from_success(0),
                _ => UserRet::from_error(ErrNo::EFAULT),
            }
        }
        SETALL => set_all(id, argument, uid, gid),
        _ => UserRet::from_error(ErrNo::EINVAL),
    }
}

fn set_all(id : i32, pointer : usize, uid : u32, gid : u32) -> UserRet {
    if pointer == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let count = {
        let registry = SEMAPHORES.lock();
        let set = match registry.by_id.get(&id) {
            Some(set) => set,
            None => return UserRet::from_error(ErrNo::EINVAL),
        };
        if !has_access(set, true, uid, gid) {
            return UserRet::from_error(ErrNo::EACCES);
        }
        set.values.len()
    };
    let mut values = Vec::new();
    if values.try_reserve_exact(count).is_err() {
        return UserRet::from_error(ErrNo::ENOMEM);
    }
    for index in 0..count {
        let address = match pointer.checked_add(index * core::mem::size_of::<u16>()) {
            Some(address) => address,
            None => return UserRet::from_error(ErrNo::EFAULT),
        };
        let value = match copy_from_user_struct::<u16>(address) {
            Ok(value) if value as i32 <= SEMVMX => value as i32,
            Ok(_) => return UserRet::from_error(ErrNo::ERANGE),
            Err(error) => return UserRet::from_error(error),
        };
        values.push(value);
    }
    let pid = process_id();
    let timestamp = now_seconds();
    let mut registry = SEMAPHORES.lock();
    let set = match registry.by_id.get_mut(&id) {
        Some(set) if set.values.len() == count => set,
        Some(_) => return UserRet::from_error(ErrNo::EINVAL),
        None => return UserRet::from_error(ErrNo::EIDRM),
    };
    set.values.copy_from_slice(&values);
    set.last_pid.fill(pid);
    set.ctime = timestamp;
    registry.undo.retain(|(_, semid, _), _| *semid != id);
    UserRet::from_success(0)
}

fn set_snapshot(set : &SemaphoreSet) -> Semid64Ds {
    Semid64Ds { perm : Ipc64Perm { key : set.key,
                                  uid : set.uid,
                                  gid : set.gid,
                                  cuid : set.cuid,
                                  cgid : set.cgid,
                                  mode : set.mode,
                                  ..Ipc64Perm::default() },
                otime : set.otime,
                ctime : set.ctime,
                nsems : set.values.len() as u64,
                ..Semid64Ds::default() }
}

/// 任务退出时回放 `SEM_UNDO`。不存在的/已删除的集合被自然忽略。
pub(crate) fn task_exit(task_id : usize) {
    let pid = task::process_task_snapshot(task_id)
        .map(|snapshot| snapshot.pid.raw().min(i32::MAX as usize) as i32)
        .unwrap_or(0);
    let timestamp = now_seconds();
    let mut registry = SEMAPHORES.lock();
    let undo = core::mem::take(&mut registry.undo);
    for ((owner, semid, number), adjustment) in undo {
        if owner != task_id {
            registry.undo.insert((owner, semid, number), adjustment);
            continue;
        }
        if let Some(set) = registry.by_id.get_mut(&semid) {
            if let Some(value) = set.values.get_mut(number as usize) {
                *value = value.saturating_add(adjustment).clamp(0, SEMVMX);
                set.last_pid[number as usize] = pid;
                set.otime = timestamp;
            }
        }
    }
}
