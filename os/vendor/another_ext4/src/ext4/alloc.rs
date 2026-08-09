use super::Ext4;
use crate::constants::*;
use crate::ext4_defs::*;
use crate::format_error;
use crate::prelude::*;
use crate::return_error;

impl Ext4 {
    fn is_group_metadata_block(&self, sb: &SuperBlock, pblock: PBlockId) -> bool {
        let inode_table_blocks =
            (sb.inodes_per_group() as usize * sb.inode_size()).div_ceil(BLOCK_SIZE) as u64;
        for id in 0..sb.block_group_count() {
            let desc = self.read_block_group(id).desc;
            if pblock == desc.block_bitmap_block() || pblock == desc.inode_bitmap_block() {
                return true;
            }
            let table = desc.inode_table_first_block();
            if pblock >= table && pblock < table + inode_table_blocks {
                return true;
            }
        }
        false
    }

    fn initialize_block_bitmap_if_needed(
        &self,
        sb: &SuperBlock,
        bg: &mut BlockGroupRef,
        bitmap: &mut Bitmap<'_>,
    ) -> Result<()> {
        if !bg.desc.has_flag(BlockGroupDesc::FLAG_BLOCK_UNINIT) {
            return Ok(());
        }

        // An uninitialized bitmap has no on-disk allocation semantics. Reconstruct a
        // conservative bitmap with exactly the descriptor's free count. Keeping the
        // low-numbered implicit overhead allocated also covers sparse-super/GDT copies.
        bitmap.set_all();
        let block_count = sb.block_count_in_group(bg.id) as usize;
        let free_target = bg.desc.get_free_blocks_count() as usize;
        let group_start = sb.first_data_block() as u64
            + bg.id as u64 * sb.blocks_per_group() as u64;
        let mut freed = 0usize;
        for local in (0..block_count).rev() {
            let pblock = group_start + local as u64;
            if self.is_group_metadata_block(sb, pblock) {
                continue;
            }
            bitmap.clear_bit(local);
            freed += 1;
            if freed == free_target {
                break;
            }
        }
        if freed != free_target {
            return_error!(
                ErrCode::EIO,
                "Cannot initialize block bitmap for group {}: free={} expected={}",
                bg.id,
                freed,
                free_target
            );
        }
        bg.desc.clear_flag(BlockGroupDesc::FLAG_BLOCK_UNINIT);
        Ok(())
    }

    fn initialize_inode_bitmap_if_needed(
        &self,
        bg: &mut BlockGroupRef,
        bitmap_data: &mut [u8],
        inode_count: usize,
    ) -> Result<()> {
        if !bg.desc.has_flag(BlockGroupDesc::FLAG_INODE_UNINIT) {
            return Ok(());
        }

        let free_target = bg.desc.free_inodes_count() as usize;
        if free_target > inode_count {
            return_error!(
                ErrCode::EIO,
                "Cannot initialize inode bitmap for group {}: free={} inodes={}",
                bg.id,
                free_target,
                inode_count
            );
        }

        // INODE_UNINIT makes the on-disk bitmap undefined. Reconstruct the logical
        // state from the descriptor, preserving any low-numbered reserved inodes.
        // Ext4 requires bits beyond the valid inode range to remain set as padding.
        let mut bitmap = Bitmap::new(bitmap_data, 8 * BLOCK_SIZE);
        bitmap.set_all();
        for local in (inode_count - free_target..inode_count).rev() {
            bitmap.clear_bit(local);
        }
        bg.desc.clear_flag(BlockGroupDesc::FLAG_INODE_UNINIT);
        Ok(())
    }

    /// Create a new inode, returning the inode and its number
    #[inline(never)]
    pub(super) fn create_inode(&self, mode: InodeMode) -> Result<InodeRef> {
        // Allocate an inode
        let is_dir = mode.file_type() == FileType::Directory;
        let id = self.alloc_inode(is_dir)?;

        // Initialize the inode
        let mut inode = Box::new(Inode::default());
        inode.set_mode(mode);
        inode.extent_init();
        let mut inode_ref = InodeRef::new(id, inode);

        // Sync the inode to disk
        self.write_inode_with_csum(&mut inode_ref);

        trace!("Alloc inode {} ok", inode_ref.id);
        Ok(inode_ref)
    }

    /// Create(initialize) the root inode of the file system
    #[inline(never)]
    pub(super) fn create_root_inode(&self) -> Result<InodeRef> {
        let mut inode = Box::new(Inode::default());
        inode.set_mode(InodeMode::from_type_and_perm(
            FileType::Directory,
            InodeMode::from_bits_retain(0o755),
        ));
        inode.extent_init();

        let mut root = InodeRef::new(EXT4_ROOT_INO, inode);
        let root_self = root.clone();

        // Add `.` and `..` entries
        self.dir_add_entry(&mut root, &root_self, ".")?;
        self.dir_add_entry(&mut root, &root_self, "..")?;
        root.inode.set_link_count(2);

        self.write_inode_with_csum(&mut root);
        Ok(root)
    }

    /// Free an allocated inode and all data blocks allocated for it
    pub(super) fn free_inode(&self, inode: &mut InodeRef) -> Result<()> {
        // Free the data blocks allocated for the inode
        let pblocks = self.extent_all_data_blocks(inode);
        self.dealloc_blocks(inode, &pblocks)?;
        // Free extent tree
        let pblocks = self.extent_all_tree_blocks(inode);
        self.dealloc_blocks(inode, &pblocks)?;
        // Free xattr block
        let xattr_block = inode.inode.xattr_block();
        if xattr_block != 0 {
            self.dealloc_blocks(inode, &[xattr_block])?;
        }
        // Deallocate the inode
        self.dealloc_inode(inode)?;
        Ok(())
    }

    /// Append a data block for an inode, return a pair of (logical block id, physical block id)
    ///
    /// Only data blocks allocated by `inode_append_block` will be counted in `inode.block_count`.
    /// Blocks allocated by calling `alloc_block` directly will not be counted, i.e., blocks
    /// allocated for the inode's extent tree.
    ///
    /// Appending a block does not increase `inode.size`, because `inode.size` records the actual
    /// size of the data content, not the number of blocks allocated for it.
    ///
    /// If the inode is a file, `inode.size` will be increased when writing to end of the file.
    /// If the inode is a directory, `inode.size` will be increased when adding a new entry to the
    /// newly created block.
    pub(super) fn inode_append_block(&self, inode: &mut InodeRef) -> Result<(LBlockId, PBlockId)> {
        let iblock = self.extent_next_lblock(inode);
        // Check the extent tree to get the physical block id
        let fblock = self.extent_query_or_create(inode, iblock, 1)?;
        self.write_inode_with_csum(inode);

        Ok((iblock, fblock))
    }

    /// Allocate a new physical block for an inode, return the physical block number
    pub(super) fn alloc_block(&self, inode: &mut InodeRef) -> Result<PBlockId> {
        let mut sb = self.read_super_block();
        let blocks_per_group = sb.blocks_per_group() as u64;
        let first_data_block = sb.first_data_block() as u64;
        // Data blocks may be allocated outside the inode's group.  The old
        // implementation used the inode group exclusively and also returned
        // the bitmap-local bit as a physical block number.
        for bgid in 0..sb.block_group_count() as BlockGroupId {
            let mut bg = self.read_block_group(bgid);
            if bg.desc.get_free_blocks_count() == 0 {
                continue;
            }
            let bitmap_block_id = bg.desc.block_bitmap_block();
            let mut bitmap_block = self.read_block(bitmap_block_id);
            let block_count = sb.block_count_in_group(bgid) as usize;
            let mut bitmap = Bitmap::new(bitmap_block.data_mut(), 8 * BLOCK_SIZE);
            self.initialize_block_bitmap_if_needed(&sb, &mut bg, &mut bitmap)?;
            let local = match bitmap.find_and_set_first_clear_bit(0, block_count) {
                Some(local) => local as u64,
                None => continue,
            };
            let fblock = first_data_block + bgid as u64 * blocks_per_group + local;
            bg.desc.set_block_bitmap_csum(&sb.uuid(), &bitmap);
            self.write_block(&bitmap_block);
            bg.desc
                .set_free_blocks_count(bg.desc.get_free_blocks_count() - 1);
            self.write_block_group_with_csum(&mut bg);
            sb.set_free_blocks_count(sb.free_blocks_count() - 1);
            self.write_super_block(&sb);
            inode.inode.add_fs_blocks(1);
            self.write_block(&Block::new(fblock, Box::new([0; BLOCK_SIZE])));
            trace!("Alloc block {} ok", fblock);
            return Ok(fblock);
        }
        return_error!(ErrCode::ENOSPC, "No free blocks in any block group");
    }

    /// Batch-deallocate physical blocks for one inode.
    ///
    /// This replaces repeated per-block updates so bitmap CRC, block-group
    /// counters and superblock counters are recomputed once per affected
    /// group instead of once per freed block.
    pub(super) fn dealloc_blocks(&self,
                                 inode: &mut InodeRef,
                                 pblocks: &[PBlockId])
                                 -> Result<()> {
        if pblocks.is_empty() {
            return Ok(());
        }
        let mut sb = self.read_super_block();
        let blocks_per_group = sb.blocks_per_group() as u64;
        let first_data_block = sb.first_data_block() as u64;
        let mut per_group: BTreeMap<BlockGroupId, Vec<usize>> = BTreeMap::new();

        for &pblock in pblocks {
            if (pblock as u64) < first_data_block {
                return_error!(ErrCode::EINVAL, "Invalid physical block {}", pblock);
            }
            let relative = pblock as u64 - first_data_block;
            let bgid = (relative / blocks_per_group) as BlockGroupId;
            let local = (relative % blocks_per_group) as usize;
            if bgid >= sb.block_group_count() ||
               local >= sb.block_count_in_group(bgid) as usize
            {
                return_error!(ErrCode::EINVAL, "Invalid physical block {}", pblock);
            }
            per_group.entry(bgid).or_default().push(local);
        }

        for (bgid, locals) in per_group {
            let mut bg = self.read_block_group(bgid);
            let bitmap_block_id = bg.desc.block_bitmap_block();
            let mut bitmap_block = self.read_block(bitmap_block_id);
            let mut bitmap = Bitmap::new(bitmap_block.data_mut(), 8 * BLOCK_SIZE);

            for &local in &locals {
                if bitmap.is_bit_clear(local) {
                    return_error!(ErrCode::EINVAL, "Block is already free");
                }
                bitmap.clear_bit(local);
                inode.inode.remove_fs_blocks(1);
            }

            bg.desc.set_block_bitmap_csum(&sb.uuid(), &bitmap);
            self.write_block(&bitmap_block);
            bg.desc
                .set_free_blocks_count(bg.desc.get_free_blocks_count() + locals.len() as u64);
            self.write_block_group_with_csum(&mut bg);
            sb.set_free_blocks_count(sb.free_blocks_count() + locals.len() as u64);
        }
        self.write_super_block(&sb);
        trace!("Freed {} block(s) for inode {}", pblocks.len(), inode.id);
        Ok(())
    }

    /// Allocate a new inode, returning the inode number.
    fn alloc_inode(&self, is_dir: bool) -> Result<InodeId> {
        let mut sb = self.read_super_block();
        let bg_count = sb.block_group_count();

        let mut bgid = 0;
        while bgid < bg_count {
            // Load block group descriptor
            let mut bg = self.read_block_group(bgid);
            // If there are no free inodes in this block group, try the next one
            if bg.desc.free_inodes_count() == 0 {
                bgid += 1;
                continue;
            }
            // Load inode bitmap
            let bitmap_block_id = bg.desc.inode_bitmap_block();
            let mut bitmap_block = self.read_block(bitmap_block_id);
            let inode_count = sb.inode_count_in_group(bgid) as usize;
            self.initialize_inode_bitmap_if_needed(
                &mut bg,
                bitmap_block.data_mut(),
                inode_count,
            )?;
            let mut bitmap = Bitmap::new(bitmap_block.data_mut(), inode_count);

            // Find a free inode
            let idx_in_bg =
                bitmap
                    .find_and_set_first_clear_bit(0, inode_count)
                    .ok_or(format_error!(
                        ErrCode::ENOSPC,
                        "No free inodes in block group {}",
                        bgid
                    ))? as u32;
            // Update bitmap in disk
            bg.desc.set_inode_bitmap_csum(&sb.uuid(), &bitmap);
            self.write_block(&bitmap_block);

            // Modify block group counters
            bg.desc
                .set_free_inodes_count(bg.desc.free_inodes_count() - 1);
            if is_dir {
                bg.desc.set_used_dirs_count(bg.desc.used_dirs_count() + 1);
            }
            let mut unused = bg.desc.itable_unused();
            let free = inode_count as u32 - unused;
            if idx_in_bg >= free {
                unused = inode_count as u32 - (idx_in_bg + 1);
                bg.desc.set_itable_unused(unused);
            }
            self.write_block_group_with_csum(&mut bg);

            // Update superblock counters
            sb.set_free_inodes_count(sb.free_inodes_count() - 1);
            self.write_super_block(&sb);

            // Compute the absolute i-node number
            let inodes_per_group = sb.inodes_per_group();
            let inode_id = bgid * inodes_per_group + (idx_in_bg + 1);
            return Ok(inode_id);
        }
        trace!("no free inode");
        return_error!(ErrCode::ENOSPC, "No free inodes in block group {}", bgid);
    }

    /// Free an inode
    fn dealloc_inode(&self, inode_ref: &mut InodeRef) -> Result<()> {
        let mut sb = self.read_super_block();

        // Calc block group id and index in block group
        let inodes_per_group = sb.inodes_per_group();
        let bgid = ((inode_ref.id - 1) / inodes_per_group) as BlockGroupId;
        let idx_in_bg = (inode_ref.id - 1) % inodes_per_group;
        // Load block group descriptor
        let mut bg = self.read_block_group(bgid);
        // Load inode bitmap
        let bitmap_block_id = bg.desc.inode_bitmap_block();
        let mut bitmap_block = self.read_block(bitmap_block_id);
        let inode_count = sb.inode_count_in_group(bgid) as usize;
        let mut bitmap = Bitmap::new(bitmap_block.data_mut(), inode_count);

        // Free the inode
        if bitmap.is_bit_clear(idx_in_bg as usize) {
            return_error!(
                ErrCode::EINVAL,
                "Inode {} is already free in block group {}",
                inode_ref.id,
                bgid
            );
        }
        bitmap.clear_bit(idx_in_bg as usize);
        let initialized = inode_count as u32 - bg.desc.itable_unused();
        if idx_in_bg + 1 == initialized {
            let mut new_initialized = idx_in_bg;
            while new_initialized > 0 && bitmap.is_bit_clear((new_initialized - 1) as usize) {
                new_initialized -= 1;
            }
            bg.desc
                .set_itable_unused(inode_count as u32 - new_initialized);
        }
        bg.desc.set_inode_bitmap_csum(&sb.uuid(), &bitmap);
        drop(bitmap);
        self.write_block(&bitmap_block);

        // Update block group counters
        bg.desc
            .set_free_inodes_count(bg.desc.free_inodes_count() + 1);
        if inode_ref.inode.is_dir() {
            bg.desc.set_used_dirs_count(bg.desc.used_dirs_count() - 1);
        }
        self.write_block_group_with_csum(&mut bg);

        // Update superblock counters
        sb.set_free_inodes_count(sb.free_inodes_count() + 1);
        self.write_super_block(&sb);

        // Clear inode content
        inode_ref.inode = Box::new(Inode::default());
        self.write_inode_with_csum(inode_ref);

        Ok(())
    }
}
