//! 文件系统操作相关的 syscall 实现。

pub(crate) mod attr;
pub(crate) mod close;
pub(crate) mod cwd;
pub(crate) mod dir;
pub(crate) mod dup;
pub(crate) mod faccessat;
pub(crate) mod fadvise;
pub(crate) mod fallocate;
pub(crate) mod fcntl;
pub(crate) mod flock;
pub(crate) mod fstat;
pub(crate) mod getdents64;
pub(crate) mod io;
pub(crate) mod inotify;
pub(crate) mod memfd;
pub(crate) mod openat;
pub(crate) mod openat2;
pub(crate) mod path_at;
pub(crate) mod pipe2;
pub(crate) mod renameat2;
pub(crate) mod sendfile;
pub(crate) mod statfs;
pub(crate) mod transfer;
pub(crate) mod truncate;
pub(crate) mod xattr;

pub(crate) use attr::{sys_fchmod, sys_fchmodat, sys_fchown, sys_fchownat, sys_utimensat};
pub(crate) use close::{sys_close, sys_close_range};
pub(crate) use cwd::{sys_chdir, sys_fchdir, sys_getcwd};
pub(crate) use dir::{
    sys_linkat, sys_mkdirat, sys_mknodat, sys_readlinkat, sys_symlinkat, sys_unlinkat,
};
pub(crate) use dup::{sys_dup, sys_dup3};
pub(crate) use faccessat::{sys_faccessat, sys_faccessat2};
pub(crate) use fadvise::{sys_fadvise64, sys_readahead};
pub(crate) use fallocate::sys_fallocate;
pub(crate) use fcntl::sys_fcntl;
pub(crate) use flock::sys_flock;
pub(crate) use fstat::{sys_fstat, sys_fstatat, sys_statx};
pub(crate) use getdents64::sys_getdents64;
pub(crate) use io::{
    sys_lseek, sys_pread64, sys_preadv, sys_preadv2, sys_pwrite64, sys_pwritev, sys_pwritev2,
    sys_read, sys_readv, sys_write, sys_writev,
};
pub(crate) use inotify::{sys_inotify_add_watch, sys_inotify_init1, sys_inotify_rm_watch};
pub(crate) use memfd::sys_memfd_create;
pub(crate) use openat::sys_openat;
pub(crate) use openat2::sys_openat2;
pub(crate) use pipe2::sys_pipe2;
pub(crate) use renameat2::{sys_renameat, sys_renameat2};
pub(crate) use sendfile::sys_sendfile;
pub(crate) use statfs::{sys_fstatfs, sys_statfs};
pub(crate) use transfer::{sys_copy_file_range, sys_splice, sys_tee, sys_vmsplice};
pub(crate) use truncate::{sys_ftruncate, sys_truncate};
pub(crate) use xattr::{
    sys_fgetxattr, sys_flistxattr, sys_fremovexattr, sys_fsetxattr, sys_getxattr, sys_lgetxattr,
    sys_listxattr, sys_llistxattr, sys_lremovexattr, sys_lsetxattr, sys_removexattr, sys_setxattr,
};

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    inotify::self_test();
    memfd::self_test();
    openat2::self_test();
    transfer::self_test();
}
