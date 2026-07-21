//! 进程间通信（IPC）相关的 syscall 实现。

pub(crate) mod futex;
pub(crate) mod shm;

pub(crate) use futex::sys_futex;
pub(crate) use shm::{sys_shmat, sys_shmctl, sys_shmdt, sys_shmget};
