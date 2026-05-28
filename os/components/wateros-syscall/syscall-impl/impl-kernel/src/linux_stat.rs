//! Linux riscv64 `struct stat` 布局（与 musl/glibc 用户态一致）。

use vfs::api::VfsMetadata;
use vfs::api::VfsNodeType;

/// riscv64 LP64 `struct stat`（128 字节）。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxStat {
    pub st_dev : u64,
    pub st_ino : u64,
    pub st_mode : u32,
    pub st_nlink : u32,
    pub st_uid : u32,
    pub st_gid : u32,
    pub st_rdev : u64,
    pub __pad1 : u64,
    pub st_size : i64,
    pub st_blksize : i32,
    pub __pad2 : i32,
    pub st_blocks : i64,
    pub st_atime_sec : i64,
    pub st_atime_nsec : i64,
    pub st_mtime_sec : i64,
    pub st_mtime_nsec : i64,
    pub st_ctime_sec : i64,
    pub st_ctime_nsec : i64,
    /// 与 oscomp `struct kstat` 尾部 `unsigned __unused[2]` 一致。
    pub __unused : [u32; 2],
}

const _: () = assert!(core::mem::size_of::<LinuxStat>() == 128);

const S_IFREG : u32 = 0o100_000;
const S_IFDIR : u32 = 0o40_000;
const S_IFCHR : u32 = 0o20_000;

pub(crate) fn fill_linux_stat(meta : &VfsMetadata, size : u64) -> LinuxStat {
    let mode = match meta.node_type {
        VfsNodeType::File => S_IFREG | (meta.mode as u32 & 0o7777),
        VfsNodeType::Directory => S_IFDIR | (meta.mode as u32 & 0o7777),
        VfsNodeType::Special => S_IFCHR | (meta.mode as u32 & 0o7777),
        _ => meta.mode as u32,
    };
    LinuxStat { st_dev : 0,
                st_ino : 1,
                st_mode : mode,
                st_nlink : 1,
                // TODO(cred-vfs): st_uid/st_gid 当前硬编码 0；后续从 VfsMetadata 读 ext4 inode owner，
                // 并结合 cred.fs_uid/fs_gid 决定 stat 返回值。
                st_uid : 0,
                st_gid : 0,
                st_rdev : 0,
                __pad1 : 0,
                st_size : size as i64,
                st_blksize : 4096,
                __pad2 : 0,
                st_blocks : ((size + 511) / 512) as i64,
                st_atime_sec : 0,
                st_atime_nsec : 0,
                st_mtime_sec : 0,
                st_mtime_nsec : 0,
                st_ctime_sec : 0,
                st_ctime_nsec : 0,
                __unused : [0; 2] }
}
