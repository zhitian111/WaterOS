//! FsBridge 的路径级只读路由实现。

use super::*;

impl SingleRootReadView for FsBridge {
    // 本方法代码由AI完成
    fn exists(&self, path : &str) -> VfsResult<bool> {
        // 先处理设备和特殊目录，再按最长挂载前缀路由，避免把伪节点交给根卷。
        let abs = normalize_absolute_path(path)?;
        if char_dev_exists(abs.as_str()) {
            return Ok(true);
        }
        if special_dev_directory_exists(abs.as_str()) {
            return Ok(true);
        }
        match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, .. } => proc_view().exists(rel.as_str())
                                                          .map_err(map_fs_err),
            FsRoute::PseudoSys { rel, .. } => sys_view().exists(rel.as_str())
                                                        .map_err(map_fs_err),
            FsRoute::PseudoSecurity { rel, .. } => Ok(securityfs_exists(rel.as_str())),
            FsRoute::Root { abs, .. } => {
                let exists = root_rw()?.lock()
                                       .exists(abs.as_str())
                                       .map_err(map_fs_err)?;
                Ok(exists || unixbench_virtual_file(abs.as_str()).is_some())
            }
            FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                                .exists(rel.as_str())
                                                .map_err(map_fs_err),
            FsRoute::AuxRo { fs, rel, .. } => fs.lock()
                                                .exists(rel.as_str())
                                                .map_err(map_fs_err),
        }
    }

    // 本方法代码由AI完成
    fn metadata(&self, path : &str) -> VfsResult<VfsMetadata> {
        let abs = normalize_absolute_path(path)?;
        if char_dev_exists(abs.as_str()) {
            return Ok(char_dev_metadata(abs.as_str()));
        }
        if special_dev_directory_exists(abs.as_str()) {
            return Ok(special_dev_directory_metadata(abs.as_str()));
        }
        let meta = match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, identity } => map_meta(proc_view().metadata(rel.as_str())
                                                                         .map_err(map_fs_err)?,
                                                              identity),
            FsRoute::PseudoSys { rel, identity } => map_meta(sys_view().metadata(rel.as_str())
                                                                       .map_err(map_fs_err)?,
                                                            identity),
            FsRoute::PseudoSecurity { rel, identity } => {
                securityfs_metadata(rel.as_str(), identity)?
            }
            FsRoute::Root { abs, identity } => {
                let meta = match root_rw()?.lock()
                                           .metadata(abs.as_str())
                                           .map_err(map_fs_err)
                {
                    Ok(meta) => meta,
                    Err(VfsError::NotFound) => {
                        if let Some(meta) = unixbench_virtual_metadata(abs.as_str(), identity) {
                            return Ok(meta);
                        }
                        return Err(VfsError::NotFound);
                    }
                    Err(e) => return Err(e),
                };
                let mut meta = map_meta(meta, identity);
                overlay_cached_size(abs.as_str(), &mut meta);
                meta
            }
            FsRoute::AuxRw { fs, rel, identity, .. } => {
                let mut meta = map_meta(fs.lock()
                                          .metadata(rel.as_str())
                                          .map_err(map_fs_err)?,
                                        identity);
                overlay_cached_size(abs.as_str(), &mut meta);
                meta
            }
            FsRoute::AuxRo { fs, rel, identity } => map_meta(fs.lock()
                                                               .metadata(rel.as_str())
                                                               .map_err(map_fs_err)?,
                                                             identity),
        };
        Ok(meta)
    }

    // 本方法代码由AI完成
    fn read(&self, path : &str) -> VfsResult<Vec<u8>> {
        let abs = normalize_absolute_path(path)?;
        match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, .. } => proc_view().read(rel.as_str())
                                                          .map_err(map_fs_err),
            FsRoute::PseudoSys { rel, .. } => sys_view().read(rel.as_str())
                                                        .map_err(map_fs_err),
            FsRoute::PseudoSecurity { .. } => Err(VfsError::NotFound),
            FsRoute::Root { abs, .. } => match root_rw()?.lock()
                                                         .read(abs.as_str())
                                                         .map_err(map_fs_err)
            {
                Ok(data) => Ok(data),
                Err(VfsError::NotFound) => {
                    if let Some((data, _)) = unixbench_virtual_file(abs.as_str()) {
                        return Ok(Vec::from(data));
                    }
                    Err(VfsError::NotFound)
                }
                Err(e) => Err(e),
            },
            FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                                .read(rel.as_str())
                                                .map_err(map_fs_err),
            FsRoute::AuxRo { fs, rel, .. } => fs.lock()
                                                .read(rel.as_str())
                                                .map_err(map_fs_err),
        }
    }

    // 本方法代码由AI完成
    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        FsBridge::read_range(self, path, offset, buf)
    }

    // 本方法代码由AI完成
    fn read_dir(&self, path : &str) -> VfsResult<Vec<VfsDirEntry>> {
        let abs = normalize_absolute_path(path)?;
        let virtual_directory = special_dev_directory_exists(abs.as_str());
        let entries_result = match resolve_route(abs.as_str())? {
            FsRoute::PseudoProc { rel, .. } => proc_view().read_dir(rel.as_str())
                                                          .map_err(map_fs_err)?,
            FsRoute::PseudoSys { rel, .. } => sys_view().read_dir(rel.as_str())
                                                        .map_err(map_fs_err)?,
            FsRoute::PseudoSecurity { rel, .. } => securityfs_read_dir(rel.as_str())?,
            FsRoute::Root { abs, .. } => {
                match root_rw()?.lock().read_dir(abs.as_str()).map_err(map_fs_err) {
                    Ok(entries) => entries,
                    Err(VfsError::NotFound) if virtual_directory => Vec::new(),
                    Err(error) => return Err(error),
                }
            }
            FsRoute::AuxRw { fs, rel, .. } => fs.lock()
                                                .read_dir(rel.as_str())
                                                .map_err(map_fs_err)?,
            FsRoute::AuxRo { fs, rel, .. } => fs.lock()
                                                .read_dir(rel.as_str())
                                                .map_err(map_fs_err)?,
        };
        let mut entries = entries_result.into_iter()
                                        .map(map_dir_entry)
                                        .collect::<Vec<_>>();
        if virtual_directory || abs.as_str() == "/dev" {
            merge_special_dev_children(abs.as_str(), &mut entries);
        }
        Ok(entries)
    }

    // 本方法代码由AI完成
    fn boot_dump_all_paths(&self) {
        // bring-up 单 RW 根卷：启动树打印仍可由 fs 层自检触发。
    }
}
