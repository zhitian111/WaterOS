//! High-level operations of Ext4 filesystem.
//!
//! This module provides path-based operations. An object can be
//! located in the filesystem by its relative or absolute path.
//!
//! Some operations such as `read`, `write`, `setattr` do not involve
//! file location. They are implemented in the `low_level` module.
//! High-level and low-level operations can be used together to
//! implement more complex operations.

use super::Ext4;
use crate::ext4_defs::*;
use crate::prelude::*;
use crate::return_error;

fn path_components(path: &str) -> impl Iterator<Item = &str> {
    let path = path.trim_start_matches('/');
    let is_root = path.is_empty();
    path.split('/').filter(move |_| !is_root)
}

fn parent_and_name(path: &str) -> Option<(&str, &str)> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return None;
    }
    Some(path.rsplit_once('/').unwrap_or(("", path)))
}

impl Ext4 {
    /// Look up an object in the filesystem recursively.
    ///
    /// # Params
    ///
    /// * `root` - The inode id of the root directory for search.
    /// * `path` - The relative path of the object to be opened.
    ///
    /// # Return
    ///
    /// `Ok(inode)` - Inode id of the object
    ///
    /// # Error
    ///
    /// * `ENOTDIR` - Any parent along `path` is not a directory.
    /// * `ENOENT` - The object does not exist.
    pub fn generic_lookup(&self, root: InodeId, path: &str) -> Result<InodeId> {
        trace!("generic_lookup({}, {})", root, path);
        // Search from the given parent inode
        let mut cur = root;
        // Search recursively
        for component in path_components(path) {
            cur = self.lookup(cur, component)?;
        }
        Ok(cur)
    }

    /// Create an object in the filesystem.
    ///
    /// This function will perform recursive-creation i.e. if the parent
    /// directory does not exist, it will be created as well.
    ///
    /// # Params
    ///
    /// * `root` - The inode id of the starting directory for search.
    /// * `path` - The relative path of the object to create.
    /// * `mode` - file mode and type to create
    ///
    /// # Return
    ///
    /// `Ok(inode)` - Inode id of the created object
    ///
    /// # Error
    ///
    /// * `ENOTDIR` - Any parent along `path` is not a directory.
    /// * `EEXIST` - The object already exists.
    pub fn generic_create(&self, root: InodeId, path: &str, mode: InodeMode) -> Result<InodeId> {
        // Search from the given parent inode
        let mut cur = self.read_inode(root);
        let mut search_path = path_components(path).peekable();
        // Search recursively
        while let Some(component) = search_path.next() {
            let is_last = search_path.peek().is_none();
            if !cur.inode.is_dir() {
                return_error!(ErrCode::ENOTDIR, "Parent {} is not a directory", cur.id);
            }
            match self.dir_find_entry(&cur, component) {
                Ok(id) => {
                    if is_last {
                        // Reach the object and it already exists
                        return_error!(
                            ErrCode::EEXIST,
                            "Object {}/{} already exists",
                            root,
                            component
                        );
                    }
                    cur = self.read_inode(id);
                }
                Err(e) => {
                    if e.code() != ErrCode::ENOENT {
                        return_error!(e.code(), "Unexpected error: {:?}", e);
                    }
                    let child_id = if is_last {
                        if mode.file_type() == FileType::Directory {
                            self.mkdir(cur.id, component, mode)?
                        } else {
                            self.create(cur.id, component, mode)?
                        }
                    } else {
                        self.mkdir(cur.id, component, InodeMode::ALL_RWX)?
                    };
                    cur = self.read_inode(child_id);
                }
            }
        }
        Ok(cur.id)
    }

    /// Remove an object from the filesystem.
    ///
    /// # Params
    ///
    /// * `root` - The inode id of the starting directory for search.
    /// * `path` - The relative path of the object to remove.
    ///
    /// # Error
    ///
    /// * `ENOENT` - The object does not exist.
    /// * `ENOTEMPTY` - The object is a non-empty directory.
    pub fn generic_remove(&self, root: InodeId, path: &str) -> Result<()> {
        // Get the parent directory path and the file name
        let Some((parent_path, file_name)) = parent_and_name(path) else {
            return_error!(ErrCode::EINVAL, "Cannot remove the lookup root");
        };
        // Get the parent directory inode
        let parent_id = self.generic_lookup(root, parent_path)?;
        // Get the child inode
        let child_id = self.lookup(parent_id, file_name)?;
        let mut parent = self.read_inode(parent_id);
        let mut child = self.read_inode(child_id);
        // Check if child is a non-empty directory
        if child.inode.is_dir() && self.dir_list_entries(&child).len() > 2 {
            return_error!(ErrCode::ENOTEMPTY, "Directory {} not empty", path);
        }
        // Unlink the file
        self.unlink_inode(&mut parent, &mut child, file_name, true)
    }

    /// Move an object from one location to another.
    ///
    /// # Params
    ///
    /// * `root` - The inode id of the starting directory for search.
    /// * `src` - The relative path of the object to move.
    /// * `dst` - The relative path of the destination.
    ///
    /// # Error
    ///
    /// * `ENOTDIR` - Any parent in the path is not a directory.
    /// * `ENOENT` - The source object does not exist.
    /// * `EEXIST` - The destination object already exists.
    pub fn generic_rename(&self, root: InodeId, src: &str, dst: &str) -> Result<()> {
        // Parse the directories and file names
        let Some((src_parent_path, src_file_name)) = parent_and_name(src) else {
            return_error!(ErrCode::EINVAL, "Cannot rename the lookup root");
        };
        let Some((dst_parent_path, dst_file_name)) = parent_and_name(dst) else {
            return_error!(ErrCode::EINVAL, "Cannot replace the lookup root");
        };
        // Get source and des inodes
        let src_parent_id = self.generic_lookup(root, src_parent_path)?;
        let dst_parent_id = self.generic_lookup(root, dst_parent_path)?;
        // Move the file
        self.rename(src_parent_id, src_file_name, dst_parent_id, dst_file_name)
    }
}

#[cfg(test)]
mod tests {
    use super::{parent_and_name, path_components};
    use crate::prelude::*;

    #[test]
    fn borrowed_components_preserve_existing_split_semantics() {
        assert_eq!(path_components("").collect::<Vec<_>>(), Vec::<&str>::new());
        assert_eq!(path_components("///").collect::<Vec<_>>(), Vec::<&str>::new());
        assert_eq!(path_components("/a/b").collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(path_components("a//b/").collect::<Vec<_>>(),
                   vec!["a", "", "b", ""]);
    }

    #[test]
    fn borrowed_parent_and_name_handles_nested_paths() {
        assert_eq!(parent_and_name(""), None);
        assert_eq!(parent_and_name("///"), None);
        assert_eq!(parent_and_name("/name"), Some(("", "name")));
        assert_eq!(parent_and_name("/a/b"), Some(("a", "b")));
        assert_eq!(parent_and_name("a//b"), Some(("a/", "b")));
        assert_eq!(parent_and_name("a/"), Some(("a", "")));
    }
}
