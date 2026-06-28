//! Linux riscv64 `struct stat` 布局（与 musl/glibc 用户态一致）。

use vfs::api::VfsMetadata;
use vfs::api::VfsNodeType;

/// riscv64 LP64 `struct stat`（128 字节）。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LinuxStat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub __pad1: u64,
    pub st_size: i64,
    pub st_blksize: i32,
    pub __pad2: i32,
    pub st_blocks: i64,
    pub st_atime_sec: i64,
    pub st_atime_nsec: i64,
    pub st_mtime_sec: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime_sec: i64,
    pub st_ctime_nsec: i64,
    /// 与 oscomp `struct kstat` 尾部 `unsigned __unused[2]` 一致。
    pub __unused: [u32; 2],
}

const _: () = assert!(core::mem::size_of::<LinuxStat>() == 128);

/// Linux `struct statx_timestamp`。
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LinuxStatxTimestamp {
    pub tv_sec: i64,
    pub tv_nsec: u32,
    pub __reserved: i32,
}

/// Linux asm-generic `struct statx`（256 字节）。
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LinuxStatx {
    pub stx_mask: u32,
    pub stx_blksize: u32,
    pub stx_attributes: u64,
    pub stx_nlink: u32,
    pub stx_uid: u32,
    pub stx_gid: u32,
    pub stx_mode: u16,
    pub __spare0: u16,
    pub stx_ino: u64,
    pub stx_size: u64,
    pub stx_blocks: u64,
    pub stx_attributes_mask: u64,
    pub stx_atime: LinuxStatxTimestamp,
    pub stx_btime: LinuxStatxTimestamp,
    pub stx_ctime: LinuxStatxTimestamp,
    pub stx_mtime: LinuxStatxTimestamp,
    pub stx_rdev_major: u32,
    pub stx_rdev_minor: u32,
    pub stx_dev_major: u32,
    pub stx_dev_minor: u32,
    pub stx_mnt_id: u64,
    pub stx_dio_mem_align: u32,
    pub stx_dio_offset_align: u32,
    pub __spare3: [u64; 12],
}

const _: () = assert!(core::mem::size_of::<LinuxStatx>() == 256);

const S_IFREG: u32 = 0o100_000;
const S_IFDIR: u32 = 0o40_000;
const S_IFCHR: u32 = 0o20_000;
const S_IFLNK: u32 = 0o120_000;
const S_IFMT: u32 = 0o170_000;

const STATX_TYPE: u32 = 0x0001;
const STATX_MODE: u32 = 0x0002;
const STATX_NLINK: u32 = 0x0004;
const STATX_INO: u32 = 0x0100;
const STATX_SIZE: u32 = 0x0200;
const STATX_BLOCKS: u32 = 0x0400;
const STATX_UID: u32 = 0x0008;
const STATX_GID: u32 = 0x0010;
const STATX_MNT_ID: u32 = 0x1000;
const STATX_SUPPORTED: u32 = STATX_TYPE
    | STATX_MODE
    | STATX_NLINK
    | STATX_UID
    | STATX_GID
    | STATX_INO
    | STATX_SIZE
    | STATX_BLOCKS
    | STATX_MNT_ID;

fn linux_mode(meta: &VfsMetadata) -> u32 {
    let raw = u32::from(meta.mode);
    if raw & S_IFMT != 0 {
        return raw;
    }
    let file_type = match meta.node_type {
        VfsNodeType::File => S_IFREG,
        VfsNodeType::Directory => S_IFDIR,
        VfsNodeType::Symlink => S_IFLNK,
        VfsNodeType::Special => S_IFCHR,
    };
    file_type | (raw & 0o7777)
}

fn linux_dev(major: u32, minor: u32) -> u64 {
    let major = u64::from(major);
    let minor = u64::from(minor);
    (minor & 0xff)
        | ((major & 0xfff) << 8)
        | ((minor & !0xff) << 12)
        | ((major & !0xfff) << 32)
}

pub(crate) fn fill_linux_stat(meta: &VfsMetadata, size: u64) -> LinuxStat {
    let mode = linux_mode(meta);
    LinuxStat {
        st_dev: linux_dev(meta.device_major, meta.device_minor),
        st_ino: meta.inode,
        st_mode: mode,
        st_nlink: meta.nlink,
        st_uid: meta.uid,
        st_gid: meta.gid,
        st_rdev: linux_dev(meta.device_major, meta.device_minor),
        __pad1: 0,
        st_size: size as i64,
        st_blksize: 4096,
        __pad2: 0,
        st_blocks: ((size + 511) / 512) as i64,
        st_atime_sec: 0,
        st_atime_nsec: 0,
        st_mtime_sec: 0,
        st_mtime_nsec: 0,
        st_ctime_sec: 0,
        st_ctime_nsec: 0,
        __unused: [0; 2],
    }
}

pub(crate) fn fill_linux_statx(meta: &VfsMetadata, size: u64, _requested_mask: u32) -> LinuxStatx {
    let mode = linux_mode(meta);
    LinuxStatx {
        stx_mask: STATX_SUPPORTED,
        stx_blksize: 4096,
        stx_nlink: meta.nlink,
        stx_uid: meta.uid,
        stx_gid: meta.gid,
        stx_mode: mode as u16,
        stx_ino: meta.inode,
        stx_size: size,
        stx_blocks: (size + 511) / 512,
        stx_rdev_major: meta.device_major,
        stx_rdev_minor: meta.device_minor,
        stx_dev_major: meta.device_major,
        stx_dev_minor: meta.device_minor,
        stx_mnt_id: meta.mount_id,
        ..LinuxStatx::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_device_encoding_matches_small_and_extended_values() {
        assert_eq!(linux_dev(8, 1), 0x801);
        assert_eq!(linux_dev(0x1234, 0x56789), 0x1000_5672_3489);
    }

    #[test]
    fn stat_and_statx_preserve_vfs_identity() {
        let meta = VfsMetadata {
            node_type: VfsNodeType::File,
            size: 4097,
            mode: 0o644,
            device_major: 8,
            device_minor: 2,
            inode: 42,
            mount_id: 7,
            nlink: 3,
            uid: 1000,
            gid: 1001,
        };
        let stat = fill_linux_stat(&meta, meta.size);
        assert_eq!(stat.st_dev, linux_dev(8, 2));
        assert_eq!(stat.st_ino, 42);
        assert_eq!(stat.st_nlink, 3);
        assert_eq!(stat.st_uid, 1000);
        assert_eq!(stat.st_gid, 1001);

        let statx = fill_linux_statx(&meta, meta.size, u32::MAX);
        assert_eq!(statx.stx_ino, 42);
        assert_eq!(statx.stx_dev_major, 8);
        assert_eq!(statx.stx_dev_minor, 2);
        assert_eq!(statx.stx_mnt_id, 7);
        assert_eq!(statx.stx_mask, STATX_SUPPORTED);
    }
}
