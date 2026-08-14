//! Path validation, inode lookup and metadata conversion.

extern crate alloc;

use another_ext4::{Ext4, InodeMode, EXT4_ROOT_INO};
use api_v0::{FsError, FsMetadata, FsResult};
use super::block_io::{map_error, map_type};

pub(crate) fn lookup(fs: &Ext4, path: &str) -> FsResult<u32> {
    if path == "/" || path.is_empty() {
        return Ok(EXT4_ROOT_INO);
    }
    if !path.starts_with('/') || path.split('/').any(|part| part == "." || part == "..") {
        return Err(FsError::InvalidPath);
    }
    fs.generic_lookup(EXT4_ROOT_INO, path).map_err(map_error)
}

pub(crate) fn metadata(fs: &Ext4, inode: u32) -> FsResult<FsMetadata> {
    let attr = fs.getattr(inode).map_err(map_error)?;
    Ok(metadata_from_attr(attr))
}

pub(crate) fn metadata_open(fs: &Ext4, inode: u32) -> FsMetadata {
    metadata_from_attr(fs.getattr_open(inode))
}

fn metadata_from_attr(attr: another_ext4::FileAttr) -> FsMetadata {
    let mode = InodeMode::from_type_and_perm(attr.ftype, attr.perm).bits();
    FsMetadata { node_type: map_type(attr.ftype), size: attr.size, mode,
                 inode: attr.ino as u64, nlink: attr.links as u32,
                 uid: attr.uid, gid: attr.gid }
}

pub(crate) fn write_with_ordered_size(fs: &Ext4, inode: u32, offset: u64, data: &[u8]) -> FsResult<usize> {
    let data_len = u64::try_from(data.len()).map_err(|_| FsError::NoSpace)?;
    let end = offset.checked_add(data_len).ok_or(FsError::NoSpace)?;
    let offset = usize::try_from(offset).map_err(|_| FsError::NoSpace)?;
    if end > fs.getattr_open(inode).size {
        fs.setattr(inode, None, None, None, Some(end), None, None, None, None).map_err(map_error)?;
        fs.flush_all();
    }
    fs.write(inode, offset, data).map_err(map_error)?;
    fs.flush_all();
    Ok(data.len())
}

pub(crate) fn parent_name(path: &str) -> FsResult<(&str, &str)> {
    let path = path.trim_end_matches('/');
    let (parent, name) = path.rsplit_once('/').ok_or(FsError::InvalidPath)?;
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." {
        return Err(FsError::InvalidPath);
    }
    Ok((if parent.is_empty() { "/" } else { parent }, name))
}
