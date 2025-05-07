use crate::fs::common::*;

// Ext4 super block
// #[repr(packed)]
pub struct Ext4SuperBlock {
    pub s_inodes_count : u32,         // Total inode count
    pub s_blocks_count_lo : u32,      // Total block count
    pub s_r_blocks_count_lo : u32,    // superuser block count
    pub s_free_blocks_count_lo : u32, // Free block count
    pub s_free_inodes_count : u32,    // Free inode count
    pub s_first_data_block : u32, // First data block (This must be at least 1 for 1k-block filesystems and is typically 0 for all other block sizes. )
    pub s_log_block_size : u32,   // Block size = 2^s_log_block_size
    pub s_log_cluster_size : u32, // Cluster size = 2^s_log_cluster_size (This is the size of the blocks that are allocated to hold file data. )
    pub s_blocks_per_group : u32, // Blocks per group
    pub s_clusters_per_group : u32, // Clusters per group
    pub s_inodes_per_group : u32, // Inodes per group
    pub s_mtime : u32, // Mount time , in seconds since the epoch (00:00:00 UTC, January 1, 1970)
    pub s_wtime : u32, // Write time , in seconds since the epoch (00:00:00 UTC, January 1, 1970)
    pub s_mnt_count : u16, // number of mounts since the last fsck
    pub s_max_mnt_count : u16, // number of mounts beyond which a fsck is needed
    pub s_magic : u16, // Magic signature ("0xEF53")

    // File system state.Valid values are:
    // 0x0001: Cleanly unmounted
    // 0x0002: Errors detected
    // 0x0004: Orphans being recovered
    pub s_state : u16,

    // Behaviour when detecting errors. One of:
    // 1: Continue
    // 2: Recover
    // 3: Panic
    pub s_errors : u16,

    pub s_minor_rev_level : u16, // Minor revision level
    pub s_lastcheck : u32, // time of last check , in seconds since the epoch (00:00:00 UTC, January 1, 1970)

    // OS, one of:
    // 0: Linux
    // 1: Hurd
    // 2: Masix
    // 3: FreeBSD
    // 4: Lites
    pub s_creator_os : u32,

    // Revision level. One of:
    // 0: The original format
    // 1: v2 format without descriptor blocks
    pub s_rev_level : u32,

    pub s_def_resuid : u16, // Default uid for reserved blocks
    pub s_def_resgid : u16, // Default gid for reserved blocks

    // These fields are for EXT4_DYNAMIC_REV superblocks only.
    //
    // Note: the difference between the compatible feature set and the incompatible feature set is that if there is a bit set in the incompatible feature set that the kernel doesn't know about, it should refuse to mount the filesystem.
    //
    // e2fsck's requirements are more strict; if it doesn't know about a feature in either the compatible or incompatible feature set, it must abort and not try to meddle with things it doesn't understand...
    //
    // (onle avaliable when s_rev_level is 1)
    pub s_first_ino : u32,      // First non-reserved inode
    pub s_inode_size : u16,     // Size of inode structure, in bytes
    pub s_block_group_nr : u16, // Block group number of this superblock

    // Compatible feature set flags. Kernel can still read/write this fs even if it doesn't understand a flag; e2fsck will not attempt to fix a filesystem with any unknown COMPAT flags. Any of:
    // 0x1 	Directory preallocation (COMPAT_DIR_PREALLOC).
    // 0x2 	"imagic inodes". Used by AFS to indicate inodes that are not linked into the directory namespace. Inodes marked with this flag will not be added to lost+found by e2fsck. (COMPAT_IMAGIC_INODES).
    // 0x4 	Has a journal (COMPAT_HAS_JOURNAL).
    // 0x8 	Supports extended attributes (COMPAT_EXT_ATTR).
    // 0x10 	Has reserved GDT blocks for filesystem expansion. Requires RO_COMPAT_SPARSE_SUPER. (COMPAT_RESIZE_INODE).
    // 0x20 	Has indexed directories. (COMPAT_DIR_INDEX).
    // 0x40 	"Lazy BG". Not in Linux kernel, seems to have been for uninitialized block groups? (COMPAT_LAZY_BG).
    // 0x80 	"Exclude inode". Intended for filesystem snapshot feature, but not used. (COMPAT_EXCLUDE_INODE).
    // 0x100 	"Exclude bitmap". Seems to be used to indicate the presence of snapshot-related exclude bitmaps? Not defined in kernel or used in e2fsprogs. (COMPAT_EXCLUDE_BITMAP).
    // 0x200 	Sparse Super Block, v2. If this flag is set, the SB field s_backup_bgs points to the two block groups that contain backup superblocks. (COMPAT_SPARSE_SUPER2).
    pub s_feature_compat : u32,

    // Incompatible feature set. If the kernel or e2fsck doesn't understand one of these bits, it will refuse to mount or attempt to repair the filesystem. Any of:
    // 0x1 	Compression. Not implemented. (INCOMPAT_COMPRESSION).
    // 0x2 	Directory entries record the file type. See ext4_dir_entry_2 below. (INCOMPAT_FILETYPE).
    // 0x4 	Filesystem needs journal recovery. (INCOMPAT_RECOVER).
    // 0x8 	Filesystem has a separate journal device. (INCOMPAT_JOURNAL_DEV).
    // 0x10 	Meta block groups. See the earlier discussion of this feature. (INCOMPAT_META_BG).
    // 0x40 	Files in this filesystem use extents. (INCOMPAT_EXTENTS).
    // 0x80 	Enable a filesystem size over 2^32 blocks. (INCOMPAT_64BIT).
    // 0x100 	Multiple mount protection. Prevent multiple hosts from mounting the filesystem concurrently by updating a reserved block periodically while mounted and checking this at mount time to determine if the filesystem is in use on another host. (INCOMPAT_MMP).
    // 0x200 	Flexible block groups. See the earlier discussion of this feature. (INCOMPAT_FLEX_BG).
    // 0x400 	Inodes can be used to store large extended attribute values (INCOMPAT_EA_INODE).
    // 0x1000 	Data in directory entry. Allow additional data fields to be stored in each dirent, after struct ext4_dirent. The presence of extra data is indicated by flags in the high bits of ext4_dirent file type flags (above EXT4_FT_MAX). The flag EXT4_DIRENT_LUFID = 0x10 is used to store a 128-bit File Identifier for Lustre. The flag EXT4_DIRENT_IO64 = 0x20 is used to store the high word of 64-bit inode numbers. Feature still in development. (INCOMPAT_DIRDATA).
    // 0x2000 	Metadata checksum seed is stored in the superblock. This feature enables the administrator to change the UUID of a metadata_csum filesystem while the filesystem is mounted; without it, the checksum definition requires all metadata blocks to be rewritten. (INCOMPAT_CSUM_SEED).
    // 0x4000 	Large directory >2GB or 3-level htree. Prior to this feature, directories could not be larger than 4GiB and could not have an htree more than 2 levels deep. If this feature is enabled, directories can be larger than 4GiB and have a maximum htree depth of 3. (INCOMPAT_LARGEDIR).
    // 0x8000 	Data in inode. Small files or directories are stored directly in the inode i_blocks and/or xattr space. (INCOMPAT_INLINE_DATA).
    // 0x10000 	Encrypted inodes are present on the filesystem (INCOMPAT_ENCRYPT).
    pub s_feature_incompat : u32,

    // Readonly-compatible feature set. If the kernel doesn't understand one of these bits, it can still mount read-only, but e2fsck will refuse to modify the filesystem. Any of:
    // 0x1 	Sparse superblocks. See the earlier discussion of this feature. (RO_COMPAT_SPARSE_SUPER).
    // 0x2 	Allow storing files larger than 2GiB (RO_COMPAT_LARGE_FILE).
    // 0x4 	Was intended for use with htree directories, but was not needed. Not used in kernel or e2fsprogs (RO_COMPAT_BTREE_DIR).
    // 0x8 	This filesystem has files whose space usage is stored in i_blocks in units of filesystem blocks, not 512-byte sectors. Inodes using this feature will be marked with EXT4_INODE_HUGE_FILE. (RO_COMPAT_HUGE_FILE)
    // 0x10 	Group descriptors have checksums. In addition to detecting corruption, this is useful for lazy formatting with uninitialized groups (RO_COMPAT_GDT_CSUM).
    // 0x20 	Indicates that the old ext3 32,000 subdirectory limit no longer applies. A directory's i_links_count will be set to 1 if it is incremented past 64,999. (RO_COMPAT_DIR_NLINK).
    // 0x40 	Indicates that large inodes exist on this filesystem, storing extra fields after EXT2_GOOD_OLD_INODE_SIZE. (RO_COMPAT_EXTRA_ISIZE).
    // 0x80 	This filesystem has a snapshot. Not implemented in ext4. (RO_COMPAT_HAS_SNAPSHOT).
    // 0x100 	Quota is handled transactionally with the journal (RO_COMPAT_QUOTA).
    // 0x200 	This filesystem supports "bigalloc", which means that filesystem block allocation bitmaps are tracked in units of clusters (of blocks) instead of blocks (RO_COMPAT_BIGALLOC).
    // 0x400 	This filesystem supports metadata checksumming. (RO_COMPAT_METADATA_CSUM; implies RO_COMPAT_GDT_CSUM, though GDT_CSUM must not be set)
    // 0x800 	Filesystem supports replicas. This feature is neither in the kernel nor e2fsprogs. (RO_COMPAT_REPLICA).
    // 0x1000 	Read-only filesystem image; the kernel will not mount this image read-write and most tools will refuse to write to the image. (RO_COMPAT_READONLY).
    // 0x2000 	Filesystem tracks project quotas. (RO_COMPAT_PROJECT)
    pub s_feature_ro_compat : u32,
    pub s_uuid : [u8; 16],             // 128-bit uuid for volume
    pub s_volume_name : [char; 16],    // Volume lable
    pub s_last_mounted : [char; 64],   // Directory where last mounted
    pub s_algorthm_usage_bitmap : u32, // For compression (not used in e2fsprogs/linux)

    // Performance hints. Directory preallocation should only happen if the EXT4_FEATURE_COMPAT_DIR_PREALLOC flag is on.
    pub s_prealloc_blocks : u8, // Number of blocks to try to preallocate
    pub s_prealloc_dir_blocks : u8, // Number of blocks to preallocate for directories
    pub s_reserved_gdt_blocks : u16, // Number of reserved GDT blocks for expansion

    // Journaling support valid if EXT4_FEATURE_COMPAT_HAS_JOURNAL set.
    pub s_journal_uuid : [u8; 16], // UUID of journal superblock
    pub s_journal_inum : u32,      // inode number of journal file
    pub s_journal_dev : u32, // device number of journal file, if the external journal feaure flag is set

    pub s_last_orphan : u32,    // start of list of orphaned inodes to delete
    pub s_hash_seed : [u32; 4], // HTREE hash seed

    // Default hash algoritm to use for directory indexing. One of:
    // 0: Legacy
    // 1: Half MD5
    // 2: Tea
    // 3: Legacy unsigned
    // 4: Half MD4 unsigned
    // 5: Tea unsigned
    pub s_def_hash_version : u8,

    pub s_jnl_backup_type : u8, // If this value is 0 or EXT3_JNL_BACKUP_BLOCKS (1), then the s_jnl_blocks field contains a duplicate copy of the inode's i_block[] array and i_size.
    pub s_desc_size : u16,      // Size of group descriptors, if the 64bit feature flag is set

    // Default mount options. Any of:
    // 0x0001   Print debugging info upon (re)mount. (EXT4_DEFM_DEBUG)
    // 0x0002 	New files take the gid of the containing directory (instead of the fsgid of the current process). (EXT4_DEFM_BSDGROUPS)
    // 0x0004 	Support userspace-provided extended attributes. (EXT4_DEFM_XATTR_USER)
    // 0x0008 	Support POSIX access control lists (ACLs). (EXT4_DEFM_ACL)
    // 0x0010 	Do not support 32-bit UIDs. (EXT4_DEFM_UID16)
    // 0x0020 	All data and metadata are commited to the journal. (EXT4_DEFM_JMODE_DATA)
    // 0x0040 	All data are flushed to the disk before metadata are committed to the journal. (EXT4_DEFM_JMODE_ORDERED)
    // 0x0060 	Data ordering is not preserved; data may be written after the metadata has been written. (EXT4_DEFM_JMODE_WBACK)
    // 0x0100 	Disable write flushes. (EXT4_DEFM_NOBARRIER)
    // 0x0200 	Track which blocks in a filesystem are metadata and therefore should not be used as data blocks. This option will be enabled by default on 3.18, hopefully. (EXT4_DEFM_BLOCK_VALIDITY)
    // 0x0400 	Enable DISCARD support, where the storage device is told about blocks becoming unused. (EXT4_DEFM_DISCARD)
    // 0x0800 	Disable delayed allocation. (EXT4_DEFM_NODELALLOC)
    pub s_default_mount_opts : u32,

    pub s_first_meta_bg : u32, // First metablock block group, if the meta_bg feature flag is enabled
    pub s_mkfs_time : u32, // When the filesystem was created, in seconds since the epoch (00:00:00 UTC, January 1, 1970)
    pub s_jnl_blocks : [u32; 17], // Backup copy of the journal inode's i_block[] array in the first 15 elements and i_size_high and i_size in the 16th and 17th elements, respectively.

    // 64bit suppot valid if EXT4_FEATURE_INCOMPAT_64BIT is set.
    pub s_blocks_count_hi : u32,      // High 32-bits of the block count
    pub s_r_blocks_count_hi : u32,    // High 32-bits of the reserved block count
    pub s_free_blocks_count_hi : u32, // High 32-bits of the free block count
    pub s_min_extra_isize : u16,      // All inodes have at least this many extra isize fields
    pub s_want_extra_isize : u16, // New inodes should reserve at least this many extra isize fields

    // Miscellaneous flages.Any of:
    // 0x0001 	Signed directory hash in use.
    // 0x0002 	Unsigned directory hash in use.
    // 0x0004 	To test development code.
    pub s_flags : u32,
    pub s_raid_stride : u16, // RAID stride. This is the number of logical blocks read from or written to the disk before moving to the next disk. This affects the placement of filesystem metadata, which will hopefully make RAID storage faster.
    pub s_mmp_interval : u16, // # seconds to wait in multi-mount prevention (MMP) checking. In theory, MMP is a mechanism to record in the superblock which host and device have mounted the filesystem, in order to prevent multiple mounts. This feature does not seem to be implemented...
    pub s_raid_stripe_width : u32, //  	RAID stripe width. This is the number of logical blocks read from or written to the disk before coming back to the current disk. This is used by the block allocator to try to reduce the number of read-modify-write operations in a RAID5/6.
    pub s_log_groups_per_flex : u8, // Size of flexible block group is 2^log_groups_per_flex.
    pub s_checksum_type : u8, // Metadata checksum algorithm type. The only valid value is 1(CRC32)
    pub s_reserved_pad : u16, // Reserved for future use.
    pub s_kbytes_written : u64, // Number of kbytes written to the filesystem.
    pub s_snapshot_inum : u32, // Inode number of the active snapshot.
    pub s_snapshot_id : u32,  // Sequential ID of active snapshot.
    pub s_snapshot_r_blocks_count : u64, // Number of blocks reserved for active snapshot's future use. (Not used in e2fsprogs/Linux.)
    pub s_snapshot_list : u32,           // Inode number of the head of the on-disk snapshot list.
    pub s_error_count : u32,             // Number of errors seen.
    pub s_first_error_time : u32,        // Time of first error.
    pub s_first_error_ino : u32,         // Inode involved in first error.
    pub s_first_error_block : u64,       // Block involved in first error.
    pub s_first_error_func : [char; 32], // Function where error occurred.
    pub s_first_error_line : u32,        // Line number where error occurred.
    pub s_last_error_time : u32,         // Time of most recent error.
    pub s_last_error_ino : u32,          // Inode involved in most recent error.
    pub s_last_error_line : u32,         // Line number where most recent error occurred.
    pub s_last_error_block : u64,        // Block involved in most recent error.
    pub s_last_error_func : [char; 32],  // Function where most recent error occurred.
    pub s_mount_opts : [char; 64],       // ASCIIZ string of mount options.
    pub s_usr_quota_inum : u32,          // Inode number of user quota file.
    pub s_grp_quota_inum : u32,          // Inode number of group quota file.
    pub s_overhead_blocks : u32, // Overhead blocks/clusters in fs. (Huh? This field is always zero, which means that the kernel calculates it dynamically.)
    pub s_backup_bgs : [u32; 2], // Block groups containing superblock backups (if sparse_super2)

    // Encryption algorithms in use. There can be up to four algorithms in use at any time; valid algorithm codes are given below:
    // 0 	Invalid algorithm (ENCRYPTION_MODE_INVALID).
    // 1 	256-bit AES in XTS mode (ENCRYPTION_MODE_AES_256_XTS).
    // 2 	256-bit AES in GCM mode (ENCRYPTION_MODE_AES_256_GCM).
    // 3 	256-bit AES in CBC mode (ENCRYPTION_MODE_AES_256_CBC).
    pub s_encrypt_algos : [u8; 4],

    pub s_encrypt_pw_salt : [u8; 16], // Salt for the string2key algorithm for encryption.
    pub s_lpf_ino : u32,              // Inode number of the lost+found directory.
    pub s_prj_quota_inum : u32,       // Inode that tracks project quotas.
    pub s_checksum_seed : u32, // Checksum seed used for metadata_csum calculations. This value is crc32c(~0, $orig_fs_uuid).
    pub s_reserved : [u32; 98], // Padding to the end of the block.
    pub s_checksum : u32,      // CRC32C checksum of the superblock.
}

// 块组描述符
pub struct Ext4BlockGroupDescriptor {
    pub bg_block_bitmap_lo : u32, // Lower 32-bits of location of block bitmap.
    pub bg_inode_bitmap_lo : u32, // Lower 32-bits of location of inode bitmap.
    pub bg_inode_table_low : u32, // Lower 32-bits of location of inode table.
    pub bg_free_blocks_count_lo : u16, // Lower 16-bits of free block count.
    pub bg_free_inodes_count_lo : u16, // Lower 16-bits of free inode count.
    pub bg_used_dirs_count_lo : u16, // Lower 16-bits of used directory count.

    // Block group flags. Any of:
    // 0x1 	inode table and bitmap are not initialized (EXT4_BG_INODE_UNINIT).
    // 0x2 	block bitmap is not initialized (EXT4_BG_BLOCK_UNINIT).
    // 0x4 	inode table is zeroed (EXT4_BG_INODE_ZEROED).
    pub bg_flags : u16,

    pub bg_exclude_bitmap_lo : u32, // Lower 32-bits of location of snapshot exclusion bitmap.
    pub bg_block_bitmap_csum_lo : u32, // Lower 32-bits of location of block bitmap checksum.
    pub bg_inode_bitmap_csum_lo : u32, // Lower 32-bits of location of inode bitmap checksum.

    pub bg_itable_unused_lo : u16, // Lower 16-bits of unused inode count. If set, we needn't scan past the (sb.s_inodes_per_group - gdt.bg_itable_unused)th entry in the inode table for this group.
    pub bg_checksum : u16, // Group descriptor checksum; crc16(sb_uuid+group+desc) if the RO_COMPAT_GDT_CSUM feature is set, or crc32c(sb_uuid+group_desc) & 0xFFFF if the RO_COMPAT_METADATA_CSUM feature is set.

    // These fields only exist if the 64bit feature is enabled and s_desc_size > 32.
    pub bg_block_bitmap_hi : u32, // Upper 32-bits of location of block bitmap.
    pub bg_inode_bitmap_hi : u32, // Upper 32-bits of location of inode bitmap.
    pub bg_inode_table_hi : u32,  // Upper 32-bits of location of inode table.
    pub bg_free_blocks_count_hi : u16, // Upper 16-bits of free block count.
    pub bg_free_inodes_count_hi : u16, // Upper 16-bits of free inode count.
    pub bg_used_dirs_count_hi : u16, // Upper 16-bits of used directory count.
    pub bg_itable_unused_hi : u16, // Upper 16-bits of unused inode count.
    pub bg_exclude_bitmap_hi : u32, // Upper 32-bits of location of snapshot exclusion bitmap.
    pub bg_block_bitmap_csum_hi : u32, // Upper 32-bits of location of block bitmap checksum.
    pub bg_inode_bitmap_csum_hi : u32, // Upper 32-bits of location of inode bitmap checksum.
    pub bg_reserved : u32,        // Padding to the end of the block.
}

// Inode 表
pub struct Ext4InodeTable {
    // File mode. Any of:
    // 0x1 	S_IXOTH (Others may execute)
    // 0x2 	S_IWOTH (Others may write)
    // 0x4 	S_IROTH (Others may read)
    // 0x8 	S_IXGRP (Group members may execute)
    // 0x10 	S_IWGRP (Group members may write)
    // 0x20 	S_IRGRP (Group members may read)
    // 0x40 	S_IXUSR (Owner may execute)
    // 0x80 	S_IWUSR (Owner may write)
    // 0x100 	S_IRUSR (Owner may read)
    // 0x200 	S_ISVTX (Sticky bit)
    // 0x400 	S_ISGID (Set GID)
    // 0x800 	S_ISUID (Set UID)
    // These are mutually-exclusive file types:
    // 0x1000 	S_IFIFO (FIFO)
    // 0x2000 	S_IFCHR (Character device)
    // 0x4000 	S_IFDIR (Directory)
    // 0x6000 	S_IFBLK (Block device)
    // 0x8000 	S_IFREG (Regular file)
    // 0xA000 	S_IFLNK (Symbolic link)
    // 0xC000 	S_IFSOCK (Socket)
    pub i_mode : u16,

    pub i_uid : u16,         // Lower 16-bits of Owner's user ID.
    pub i_size_lo : u32,     // Lower 32-bits of Size in bytes.
    pub i_atime : u32, // Last access time, in seconds since the epoch. However, if the EA_INODE inode flag is set, this inode stores an extended attribute value and this field contains the checksum of the value.
    pub i_ctime : u32, // Last inode change time, in seconds since the epoch. However, if the EA_INODE inode flag is set, this inode stores an extended attribute value and this field contains the lower 32 bits of the attribute value's reference count.
    pub i_mtime : u32, // Last data modification time, in seconds since the epoch. However, if the EA_INODE inode flag is set, this inode stores an extended attribute value and this field contains the number of the inode that owns the extended attribute.
    pub i_dtime : u32, // Deletion time, in seconds since the epoch.
    pub i_gid : u16,   // Lower 16-bits of Group ID.
    pub i_links_count : u16, // Hard link count. Normally, ext4 does not permit an inode to have more than 65,000 hard links. This applies to files as well as directories, which means that there cannot be more than 64,998 subdirectories in a directory (each subdirectory's '..' entry counts as a hard link, as does the '.' entry in the directory itself). With the DIR_NLINK feature enabled, ext4 supports more than 64,998 subdirectories by setting this field to 1 to indicate that the number of hard links is not known.
    pub i_blocks_lo : u32, // Lower 32-bits of "block" count. If the huge_file feature flag is not set on the filesystem, the file consumes i_blocks_lo 512-byte blocks on disk. If huge_file is set and EXT4_HUGE_FILE_FL is NOT set in inode.i_flags, then the file consumes i_blocks_lo + (i_blocks_hi << 32) 512-byte blocks on disk. If huge_file is set and EXT4_HUGE_FILE_FL IS set in inode.i_flags, then this file consumes (i_blocks_lo + i_blocks_hi << 32) filesystem blocks on disk.

    // Inode flags. Any of:
    // 0x1 	This file requires secure deletion (EXT4_SECRM_FL). (not implemented)
    // 0x2 	This file should be preserved, should undeletion be desired (EXT4_UNRM_FL). (not implemented)
    // 0x4 	File is compressed (EXT4_COMPR_FL). (not really implemented)
    // 0x8 	All writes to the file must be synchronous (EXT4_SYNC_FL).
    // 0x10 	File is immutable (EXT4_IMMUTABLE_FL).
    // 0x20 	File can only be appended (EXT4_APPEND_FL).
    // 0x40 	The dump(1) utility should not dump this file (EXT4_NODUMP_FL).
    // 0x80 	Do not update access time (EXT4_NOATIME_FL).
    // 0x100 	Dirty compressed file (EXT4_DIRTY_FL). (not used)
    // 0x200 	File has one or more compressed clusters (EXT4_COMPRBLK_FL). (not used)
    // 0x400 	Do not compress file (EXT4_NOCOMPR_FL). (not used)
    // 0x800 	Encrypted inode (EXT4_ENCRYPT_FL). This bit value previously was EXT4_ECOMPR_FL (compression error), which was never used.
    // 0x1000 	Directory has hashed indexes (EXT4_INDEX_FL).
    // 0x2000 	AFS magic directory (EXT4_IMAGIC_FL).
    // 0x4000 	File data must always be written through the journal (EXT4_JOURNAL_DATA_FL).
    // 0x8000 	File tail should not be merged (EXT4_NOTAIL_FL). (not used by ext4)
    // 0x10000 	All directory entry data should be written synchronously (see dirsync) (EXT4_DIRSYNC_FL).
    // 0x20000 	Top of directory hierarchy (EXT4_TOPDIR_FL).
    // 0x40000 	This is a huge file (EXT4_HUGE_FILE_FL).
    // 0x80000 	Inode uses extents (EXT4_EXTENTS_FL).
    // 0x200000 	Inode stores a large extended attribute value in its data blocks (EXT4_EA_INODE_FL).
    // 0x400000 	This file has blocks allocated past EOF (EXT4_EOFBLOCKS_FL). (deprecated)
    // 0x01000000 	Inode is a snapshot (EXT4_SNAPFILE_FL). (not in mainline)
    // 0x04000000 	Snapshot is being deleted (EXT4_SNAPFILE_DELETED_FL). (not in mainline)
    // 0x08000000 	Snapshot shrink has completed (EXT4_SNAPFILE_SHRUNK_FL). (not in mainline)
    // 0x10000000 	Inode has inline data (EXT4_INLINE_DATA_FL).
    // 0x20000000 	Create children with the same project ID (EXT4_PROJINHERIT_FL).
    // 0x80000000 	Reserved for ext4 library (EXT4_RESERVED_FL).
    // Aggregate flags:
    // 0x4BDFFF 	User-visible flags.
    // 0x4B80FF 	User-modifiable flags. Note that while EXT4_JOURNAL_DATA_FL and EXT4_EXTENTS_FL can be set with setattr, they are not in the kernel's EXT4_FL_USER_MODIFIABLE mask, since it needs to handle the setting of these flags in a special manner and they are masked out of the set of flags that are saved directly to i_flags.
    pub i_flags : u32,

    pub i_osd1 : u32, // Reserved for ext4 library.

    pub i_block : [u32; 15], // Block numbers of the disk blocks containing the file's data. The first block is the one that contains the superblock. The last block is the one that contains the block group descriptor.
    pub i_generation : u32,  // File version (for NFS).
    pub i_file_acl_lo : u32, // Lower 32-bits of extended attribute block. ACLs are of course one of many possible extended attributes; I think the name of this field is a result of the first use of extended attributes being for ACLs.
    pub i_size_high : u32, // Upper 32-bits of Size in bytes. 	Upper 32-bits of file/directory size. In ext2/3 this field was named i_dir_acl, though it was usually set to zero and never used.
    pub i_obso_faddr : u32, // Obsoleted fragment address.
    pub i_osd2 : [u16; 6], // Reserved for ext4 library.
    pub i_extra_isize : u16, // Size of this inode - 128. Alternately, the size of the extended inode fields beyond the original ext2 inode, including this field.
    pub i_checksum_hi : u16, // Upper 16-bits of the inode's checksum.
    pub i_ctime_extra : u32, // Extra change time bits. This provides sub-second precision. See Inode Timestamps section.
    pub i_mtime_extra : u32, // Extra modification time bits. This provides sub-second precision.
    pub i_atime_extra : u32, // Extra access time bits. This provides sub-second precision.
    pub i_crtime : u32, // File creation time, in seconds since the epoch. This field is used by the ext4 journaling system to record the time of the last change to the inode.
    pub i_crtime_extra : u32, // Extra creation time bits. This provides sub-second precision.
    pub i_version_hi : u32, // Upper 32-bits of the inode's version number.
    pub i_projid : u32, // Project ID. This field is used by the ext4 project quota feature to track which project a file belongs to.
}
