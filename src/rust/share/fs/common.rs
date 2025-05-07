// 文件系统类型定义
// struct FilesystemType {
//     name: &'static str,
//     magic: u32,
//     version: u32,
//     mount_flags: u32,
//     fs_flags: u32,
//     root_inode: u64,
//     block_size: u32,
//     block_count: u64,
//     block_group_count: u32,
//     inode_size: u16,
//     inode_count: u64,
//     inode_bitmap_offset: u64,
//     inode_table_offset: u64,
//     superblock_offset: u64,
//     group_desc_offset: u64,
//     journal_dev: u64,
//     journal_inode: u64,
//     journal_superblock: u64,
//     journal_block_count: u32,
//     journal_max_batch: u32,
//     journal_max_commit_age: u32,
//     journal_max_trans_age: u32,
//     journal_dirty_ratio: u8,
//     journal_ratio_min: u8,
//     journal_ratio_max: u8,
//     journal_nr_max: u32,
//     journal_commit_interval: u32,
//     journal_writeback_interval: u32,
//     journal_dio_enabled: bool,
//     reserved: [u8; 40],
// }
pub struct FilesystemType {
    pub name : &'static str,
    pub mount : fn(device : &str, flags : u64),
}

// 文件系统实例
pub struct FileSystem {
    pub root : Vnode,
}

// 文件系统节点
pub struct Vnode {
    pub inode : u64,
    pub fs : *mut FileSystem,
}

// 挂载点
pub struct Mount {
    pub fs : *mut FileSystem,
    pub mount_point : *mut Vnode,
}
