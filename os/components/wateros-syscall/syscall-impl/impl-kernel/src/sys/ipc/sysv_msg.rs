//! SysV 消息队列。
//!
//! 队列元数据由一个短全局锁保护；用户复制和阻塞等待始终在锁外完成。
//! 接收者先用 task ID 预留消息，复制失败会撤销预留，因此两个 CPU 不会取走
//! 同一条消息，也不会因 `EFAULT` 丢消息。

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use api_v0::{ErrNo, SyscallArgs, UserRet};
use spin::Mutex;

use crate::fallible_buf::{try_kbuf, SYSCALL_IO_MAX};
use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user};

const IPC_PRIVATE : i32 = 0;
const IPC_CREAT : usize = 0o1000;
const IPC_EXCL : usize = 0o2000;
const IPC_NOWAIT : usize = 0o4000;
const IPC_RMID : usize = 0;
const IPC_SET : usize = 1;
const IPC_STAT : usize = 2;
const IPC_64 : usize = 0x100;
const MSG_NOERROR : usize = 0o10000;
const MSG_EXCEPT : usize = 0o20000;
const MSG_COPY : usize = 0o40000;
const MSGMAX : usize = 8 * 1024;
const MSGMNB : usize = 16 * 1024;

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
struct Msqid64Ds {
    perm : Ipc64Perm,
    stime : i64,
    rtime : i64,
    ctime : i64,
    cbytes : u64,
    qnum : u64,
    qbytes : u64,
    lspid : i32,
    lrpid : i32,
    unused4 : u64,
    unused5 : u64,
}

const _ : () = assert!(core::mem::size_of::<Ipc64Perm>() == 48);
const _ : () = assert!(core::mem::size_of::<Msqid64Ds>() == 120);

struct Message {
    sequence : u64,
    kind : i64,
    data : Vec<u8>,
    reserved_by : Option<usize>,
}

struct MessageQueue {
    key : i32,
    uid : u32,
    gid : u32,
    cuid : u32,
    cgid : u32,
    mode : u32,
    qbytes : usize,
    bytes : usize,
    messages : VecDeque<Message>,
    stime : i64,
    rtime : i64,
    ctime : i64,
    lspid : i32,
    lrpid : i32,
}

struct MessageRegistry {
    next_id : i32,
    next_sequence : u64,
    by_id : BTreeMap<i32, MessageQueue>,
    by_key : BTreeMap<i32, i32>,
}

impl MessageRegistry {
    const fn new() -> Self {
        Self { next_id : 1,
               next_sequence : 1,
               by_id : BTreeMap::new(),
               by_key : BTreeMap::new() }
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

    fn alloc_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        sequence
    }
}

static MESSAGE_REGISTRY : Mutex<MessageRegistry> = Mutex::new(MessageRegistry::new());

fn now_seconds() -> i64 {
    platform::wall_clock::realtime_ns()
        .map(|ns| (ns / 1_000_000_000).min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

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

fn has_access(queue : &MessageQueue, write : bool, uid : u32, gid : u32) -> bool {
    if uid == 0 {
        return true;
    }
    let shift = if uid == queue.uid || uid == queue.cuid {
        6
    } else if gid == queue.gid || gid == queue.cgid {
        3
    } else {
        0
    };
    let required = if write { 0o2 } else { 0o4 };
    ((queue.mode >> shift) & required) != 0
}

fn pause_for_ipc() -> Result<(), ErrNo> {
    match task::sleep_for_ticks(1) {
        task::TaskWaitResult::Interrupted => Err(ErrNo::EINTR),
        _ => Ok(()),
    }
}

pub(crate) fn sys_msgget(args : SyscallArgs) -> UserRet {
    let key = args.arg(0) as i32;
    let flags = args.arg(1);
    let (uid, gid) = identity();
    let timestamp = now_seconds();
    let result = {
        let mut registry = MESSAGE_REGISTRY.lock();
        if key != IPC_PRIVATE {
            if let Some(id) = registry.by_key.get(&key).copied() {
                if flags & IPC_CREAT != 0 && flags & IPC_EXCL != 0 {
                    Err(ErrNo::EEXIST)
                } else {
                    let queue = registry.by_id.get(&id).ok_or(ErrNo::EINVAL);
                    queue.and_then(|queue| {
                        let requested = flags & 0o666;
                        let read_ok = requested & 0o444 == 0 || has_access(queue, false, uid, gid);
                        let write_ok = requested & 0o222 == 0 || has_access(queue, true, uid, gid);
                        if read_ok && write_ok { Ok(id) } else { Err(ErrNo::EACCES) }
                    })
                }
            } else if flags & IPC_CREAT == 0 {
                Err(ErrNo::ENOENT)
            } else {
                create_queue(&mut registry, key, flags, uid, gid, timestamp)
            }
        } else {
            create_queue(&mut registry, key, flags, uid, gid, timestamp)
        }
    };
    match result {
        Ok(id) => UserRet::from_success(id as usize),
        Err(error) => UserRet::from_error(error),
    }
}

fn create_queue(registry : &mut MessageRegistry,
                key : i32,
                flags : usize,
                uid : u32,
                gid : u32,
                timestamp : i64)
                -> Result<i32, ErrNo> {
    let id = registry.alloc_id()?;
    registry.by_id.insert(id,
                          MessageQueue { key,
                                         uid,
                                         gid,
                                         cuid : uid,
                                         cgid : gid,
                                         mode : (flags & 0o777) as u32,
                                         qbytes : MSGMNB,
                                         bytes : 0,
                                         messages : VecDeque::new(),
                                         stime : 0,
                                         rtime : 0,
                                         ctime : timestamp,
                                         lspid : 0,
                                         lrpid : 0 });
    if key != IPC_PRIVATE {
        registry.by_key.insert(key, id);
    }
    Ok(id)
}

pub(crate) fn sys_msgsnd(args : SyscallArgs) -> UserRet {
    let id = args.arg(0) as i32;
    let msgp = args.arg(1);
    let size = args.arg(2);
    let flags = args.arg(3);
    if flags & !IPC_NOWAIT != 0 || msgp == 0 || size > MSGMAX {
        return UserRet::from_error(if size > MSGMAX { ErrNo::EINVAL } else {
            ErrNo::EINVAL
        });
    }
    let kind = match copy_from_user_struct::<i64>(msgp) {
        Ok(kind) if kind > 0 => kind,
        Ok(_) => return UserRet::from_error(ErrNo::EINVAL),
        Err(error) => return UserRet::from_error(error),
    };
    let mut data = match try_kbuf(size, SYSCALL_IO_MAX) {
        Ok(data) => data,
        Err(error) => return UserRet::from_error(error),
    };
    if size != 0 {
        let data_ptr = match msgp.checked_add(core::mem::size_of::<i64>()) {
            Some(pointer) => pointer,
            None => return UserRet::from_error(ErrNo::EFAULT),
        };
        match copy_from_user(&mut data, data_ptr) {
            Ok(copied) if copied == size => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
    }
    let mut data = Some(data);
    let mut observed = false;
    let (uid, gid) = identity();
    let sender_pid = process_id();
    loop {
        let timestamp = now_seconds();
        let attempt = {
            let mut registry = MESSAGE_REGISTRY.lock();
            let Some(queue) = registry.by_id.get(&id) else {
                return UserRet::from_error(if observed { ErrNo::EIDRM } else { ErrNo::EINVAL });
            };
            observed = true;
            if !has_access(queue, true, uid, gid) {
                return UserRet::from_error(ErrNo::EACCES);
            }
            if size > queue.qbytes {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            if queue.bytes.saturating_add(size) > queue.qbytes {
                None
            } else {
                let sequence = registry.alloc_sequence();
                let queue = registry.by_id.get_mut(&id).expect("queue checked above");
                queue.bytes += size;
                queue.stime = timestamp;
                queue.lspid = sender_pid;
                queue.messages.push_back(Message { sequence,
                                                   kind,
                                                   data : data.take()
                                                              .expect("message payload moved once"),
                                                   reserved_by : None });
                Some(())
            }
        };
        if attempt.is_some() {
            return UserRet::from_success(0);
        }
        if flags & IPC_NOWAIT != 0 {
            return UserRet::from_error(ErrNo::EAGAIN);
        }
        if let Err(error) = pause_for_ipc() {
            return UserRet::from_error(error);
        }
    }
}

fn message_matches(kind : i64, requested : i64, flags : usize) -> bool {
    if requested == 0 {
        true
    } else if requested > 0 {
        if flags & MSG_EXCEPT != 0 { kind != requested } else { kind == requested }
    } else {
        kind <= requested.saturating_neg()
    }
}

fn select_message(queue : &MessageQueue, requested : i64, flags : usize) -> Option<usize> {
    if requested < 0 {
        queue.messages.iter()
             .enumerate()
             .filter(|(_, message)| message.reserved_by.is_none() &&
                                      message_matches(message.kind, requested, flags))
             .min_by_key(|(_, message)| message.kind)
             .map(|(index, _)| index)
    } else {
        queue.messages.iter()
             .position(|message| message.reserved_by.is_none() &&
                                  message_matches(message.kind, requested, flags))
    }
}

pub(crate) fn sys_msgrcv(args : SyscallArgs) -> UserRet {
    let id = args.arg(0) as i32;
    let msgp = args.arg(1);
    let capacity = args.arg(2);
    let requested = args.arg(3) as i64;
    let flags = args.arg(4);
    let known = IPC_NOWAIT | MSG_NOERROR | MSG_EXCEPT | MSG_COPY;
    if msgp == 0 || capacity > SYSCALL_IO_MAX || flags & !known != 0 ||
       flags & MSG_COPY != 0 ||
       (flags & MSG_EXCEPT != 0 && requested <= 0)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let receiver = match task_id() {
        Ok(task_id) => task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let (uid, gid) = identity();
    let receiver_pid = process_id();
    let mut observed = false;
    loop {
        let reserved = {
            let mut registry = MESSAGE_REGISTRY.lock();
            let Some(queue) = registry.by_id.get_mut(&id) else {
                return UserRet::from_error(if observed { ErrNo::EIDRM } else { ErrNo::EINVAL });
            };
            observed = true;
            if !has_access(queue, false, uid, gid) {
                return UserRet::from_error(ErrNo::EACCES);
            }
            let Some(index) = select_message(queue, requested, flags) else {
                if flags & IPC_NOWAIT != 0 {
                    return UserRet::from_error(ErrNo::ENOMSG);
                }
                drop(registry);
                if let Err(error) = pause_for_ipc() {
                    return UserRet::from_error(error);
                }
                continue;
            };
            let message = &mut queue.messages[index];
            if message.data.len() > capacity && flags & MSG_NOERROR == 0 {
                return UserRet::from_error(ErrNo::E2BIG);
            }
            message.reserved_by = Some(receiver);
            (message.sequence,
             message.kind,
             message.data[..message.data.len().min(capacity)].to_vec())
        };

        let total = core::mem::size_of::<i64>() + reserved.2.len();
        let mut output = match try_kbuf(total, SYSCALL_IO_MAX + core::mem::size_of::<i64>()) {
            Ok(output) => output,
            Err(error) => {
                unreserve_message(id, reserved.0, receiver);
                return UserRet::from_error(error);
            }
        };
        output[..8].copy_from_slice(&reserved.1.to_ne_bytes());
        output[8..].copy_from_slice(&reserved.2);
        if copy_to_user(msgp, &output).ok() != Some(output.len()) {
            unreserve_message(id, reserved.0, receiver);
            return UserRet::from_error(ErrNo::EFAULT);
        }

        let timestamp = now_seconds();
        let removed = {
            let mut registry = MESSAGE_REGISTRY.lock();
            let Some(queue) = registry.by_id.get_mut(&id) else {
                return UserRet::from_error(ErrNo::EIDRM);
            };
            let Some(index) = queue.messages.iter()
                                   .position(|message| message.sequence == reserved.0 &&
                                                      message.reserved_by == Some(receiver))
            else {
                return UserRet::from_error(ErrNo::EIDRM);
            };
            let message = queue.messages.remove(index).expect("selected message exists");
            queue.bytes = queue.bytes.saturating_sub(message.data.len());
            queue.rtime = timestamp;
            queue.lrpid = receiver_pid;
            reserved.2.len()
        };
        return UserRet::from_success(removed);
    }
}

fn unreserve_message(id : i32, sequence : u64, receiver : usize) {
    if let Some(message) = MESSAGE_REGISTRY.lock()
                                           .by_id
                                           .get_mut(&id)
                                           .and_then(|queue| queue.messages.iter_mut()
                                                                          .find(|message| {
                                                                              message.sequence ==
                                                                              sequence
                                                                          }))
    {
        if message.reserved_by == Some(receiver) {
            message.reserved_by = None;
        }
    }
}

pub(crate) fn sys_msgctl(args : SyscallArgs) -> UserRet {
    let id = args.arg(0) as i32;
    let command = args.arg(1) & !IPC_64;
    let pointer = args.arg(2);
    let (uid, gid) = identity();
    let result = match command {
        IPC_RMID => {
            let mut registry = MESSAGE_REGISTRY.lock();
            let Some(queue) = registry.by_id.get(&id) else {
                return UserRet::from_error(ErrNo::EINVAL);
            };
            if uid != 0 && uid != queue.uid && uid != queue.cuid {
                return UserRet::from_error(ErrNo::EPERM);
            }
            let queue = registry.by_id.remove(&id).expect("queue checked above");
            if queue.key != IPC_PRIVATE {
                registry.by_key.remove(&queue.key);
            }
            Ok(0)
        }
        IPC_STAT => {
            if pointer == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let snapshot = {
                let registry = MESSAGE_REGISTRY.lock();
                let queue = registry.by_id.get(&id).ok_or(ErrNo::EINVAL);
                queue.and_then(|queue| {
                    if !has_access(queue, false, uid, gid) {
                        return Err(ErrNo::EACCES);
                    }
                    Ok(queue_snapshot(queue))
                })
            };
            snapshot.and_then(|snapshot| {
                crate::user_copy::copy_to_user_struct(pointer, &snapshot).map(|_| 0)
            })
        }
        IPC_SET => {
            if pointer == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let update = match copy_from_user_struct::<Msqid64Ds>(pointer) {
                Ok(update) => update,
                Err(error) => return UserRet::from_error(error),
            };
            let timestamp = now_seconds();
            let mut registry = MESSAGE_REGISTRY.lock();
            let queue = match registry.by_id.get_mut(&id) {
                Some(queue) => queue,
                None => return UserRet::from_error(ErrNo::EINVAL),
            };
            if uid != 0 && uid != queue.uid && uid != queue.cuid {
                return UserRet::from_error(ErrNo::EPERM);
            }
            if update.qbytes == 0 || update.qbytes > usize::MAX as u64 ||
               (update.qbytes as usize > MSGMNB && uid != 0)
            {
                return UserRet::from_error(ErrNo::EPERM);
            }
            queue.uid = update.perm.uid;
            queue.gid = update.perm.gid;
            queue.mode = (queue.mode & !0o777) | (update.perm.mode & 0o777);
            queue.qbytes = update.qbytes as usize;
            queue.ctime = timestamp;
            Ok(0)
        }
        _ => Err(ErrNo::EINVAL),
    };
    match result {
        Ok(value) => UserRet::from_success(value),
        Err(error) => UserRet::from_error(error),
    }
}

fn queue_snapshot(queue : &MessageQueue) -> Msqid64Ds {
    Msqid64Ds { perm : Ipc64Perm { key : queue.key,
                                  uid : queue.uid,
                                  gid : queue.gid,
                                  cuid : queue.cuid,
                                  cgid : queue.cgid,
                                  mode : queue.mode,
                                  ..Ipc64Perm::default() },
                stime : queue.stime,
                rtime : queue.rtime,
                ctime : queue.ctime,
                cbytes : queue.bytes as u64,
                qnum : queue.messages.len() as u64,
                qbytes : queue.qbytes as u64,
                lspid : queue.lspid,
                lrpid : queue.lrpid,
                ..Msqid64Ds::default() }
}
