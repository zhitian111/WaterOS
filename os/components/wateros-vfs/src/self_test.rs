//! VFS 组合层自检：只覆盖内核态资源，并在测试后释放临时句柄与文件。

extern crate alloc;

    use alloc::string::String;

    use super::active_impl;
    use super::api::{
        SingleRootReadView, VfsDevInventory, VfsFsKind, VfsMountOps, VfsOpenFlags, VfsOpenOps,
        VfsResult, VfsSeekWhence, validate_root_file_name,
    };

    /// RW 写入后通过同一根 RW 视图读回校验。
    pub fn rw_write_root_verify(
        kind: VfsFsKind,
        name: &str,
        data: &[u8],
    ) -> VfsResult<()> {
        validate_root_file_name(name)?;
        let backend = active_impl::backend();
        let mut session = backend.mount_rw_session(kind)?;
        session.write_regular_file_at_root(name, data)?;
        let ro = backend;
        let mut path = String::from("/");
        path.push_str(name);
        let bytes = ro.read(path.as_str())?;
        if bytes.as_slice() == data {
            Ok(())
        } else {
            Err(super::api::VfsError::Io)
        }
    }

    /// RW `mkdir` 后同一 RW 视图 `metadata` 校验为目录。
    #[cfg(feature = "bridge-fs-api")]
    pub fn rw_mkdir_verify(kind: VfsFsKind, dir_name: &str) -> VfsResult<()> {
        use super::api::VfsNodeType;

        validate_root_file_name(dir_name)?;
        let backend = active_impl::backend();
        let mut path = String::from("/");
        path.push_str(dir_name);
        let mut session = backend.mount_rw_session(kind)?;
        session.mkdir(path.as_str(), 0o755)?;
        let meta = backend.metadata(path.as_str())?;
        if meta.node_type != VfsNodeType::Directory {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// `read_at` / `write_at` 不改变顺序读偏移。
    #[cfg(feature = "bridge-fs-api")]
    pub fn read_at_write_at_smoke() -> VfsResult<()> {

        const NAME: &str = "vfs_at_io_smoke";
        let mut path = String::from("/");
        path.push_str(NAME);
        let backend = active_impl::backend();
        let mut handle = backend.open(
            path.as_str(),
            VfsOpenFlags(VfsOpenFlags::READ | VfsOpenFlags::WRITE | VfsOpenFlags::CREATE),
        )?;
        handle.write_at(0, b"hello")?;
        handle.write_at(5, b" world")?;
        let _ = handle.seek(0, VfsSeekWhence::Set)?;
        let mut buf = [0u8; 11];
        let n = handle.read_at(0, &mut buf)?;
        if n != 11 || &buf != b"hello world" {
            return Err(super::api::VfsError::Io);
        }
        let mut seq = [0u8; 2];
        let n2 = handle.read(&mut seq)?;
        if n2 != 2 || &seq != b"he" {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// `/dev/null`：devfs 绑定与元数据（启动期无用户任务，不测 open/fd）。
    #[cfg(feature = "bridge-fs-api")]
    pub fn null_dev_smoke() -> VfsResult<()> {
        let backend = active_impl::backend();
        if !backend.exists("/dev/null")? {
            return Err(super::api::VfsError::NotFound);
        }
        let meta = backend.metadata("/dev/null")?;
        if meta.mode != 0o20666 {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// `open` → `read` → `seek` → `metadata` 烟囱（依赖 RW 先写入测试文件）。
    #[cfg(feature = "bridge-fs-api")]
    pub fn open_read_seek_smoke() -> VfsResult<()> {
        const NAME: &str = "vfs_open_smoke";
        const DATA: &[u8] = b"open-smoke";
        rw_write_root_verify(VfsFsKind::Ext4, NAME, DATA)?;
        let mut path = String::from("/");
        path.push_str(NAME);
        let backend = active_impl::backend();
        let mut handle = backend.open(path.as_str(), VfsOpenFlags::read())?;
        let mut buf = [0u8; 16];
        let n = handle.read(&mut buf)?;
        if &buf[..n] != DATA {
            return Err(super::api::VfsError::Io);
        }
        let _ = handle.seek(0, VfsSeekWhence::Set)?;
        let m = handle.metadata()?;
        if m.size != DATA.len() as u64 {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// `dup`/`fork` concrete wrapper 共享 OFD offset/status，独立 `open` 不共享。
    #[cfg(feature = "bridge-fs-api")]
    pub fn open_description_sharing_smoke() -> VfsResult<()> {
        const NAME : &str = "vfs_ofd_smoke";
        const DATA : &[u8] = b"abcd";
        const O_NONBLOCK : u32 = 0o4000;
        rw_write_root_verify(VfsFsKind::Ext4, NAME, DATA)?;
        let mut path = String::from("/");
        path.push_str(NAME);
        let backend = active_impl::backend();
        let mut first = backend.open(path.as_str(), VfsOpenFlags::read())?;
        let mut duplicate = first.duplicate()?;

        let mut byte = [0u8; 1];
        if first.read(&mut byte)? != 1 || byte[0] != b'a' {
            return Err(super::api::VfsError::Io);
        }
        if duplicate.read(&mut byte)? != 1 || byte[0] != b'b' {
            return Err(super::api::VfsError::Io);
        }
        duplicate.set_open_status_flags(O_NONBLOCK)?;
        if first.open_status_flags() & O_NONBLOCK == 0 {
            return Err(super::api::VfsError::Io);
        }
        duplicate.close()?;
        if first.read(&mut byte)? != 1 || byte[0] != b'c' {
            return Err(super::api::VfsError::Io);
        }

        let mut independent = backend.open(path.as_str(), VfsOpenFlags::read())?;
        if independent.read(&mut byte)? != 1 || byte[0] != b'a' {
            return Err(super::api::VfsError::Io);
        }
        Ok(())
    }

    /// Prepared read 仅按 user-copy 进度提交，并在 Drop 时取消 reservation。
    #[cfg(feature = "bridge-fs-api")]
    pub fn prepared_read_smoke() -> VfsResult<()> {
        use super::api::{VfsCopyProgress, VfsError, VfsReadFinish};

        let backend = active_impl::backend();
        let mut handle =
            backend.open("/vfs_ofd_smoke",
                         VfsOpenFlags(VfsOpenFlags::READ | VfsOpenFlags::WRITE))?;
        let mut duplicate = handle.duplicate()?;

        let lease = handle.prepare_read(3)?.acquire()?;
        if lease.bytes() != b"abc" {
            return Err(VfsError::Io);
        }
        if !matches!(duplicate.prepare_read(1), Err(VfsError::Busy)) ||
           duplicate.seek(0, VfsSeekWhence::Cur) != Err(VfsError::Busy) ||
           duplicate.write(b"q") != Err(VfsError::Busy)
        {
            return Err(VfsError::Io);
        }
        let mut independent = backend.open("/vfs_ofd_smoke", VfsOpenFlags::read())?;
        let independent_lease = independent.prepare_read(1)?.acquire()?;
        if independent_lease.bytes() != b"a" {
            return Err(VfsError::Io);
        }
        drop(independent_lease);
        if lease.finish(VfsCopyProgress { copied : 1,
                                          complete : false })? !=
           VfsReadFinish::Bytes(1)
        {
            return Err(VfsError::Io);
        }
        if duplicate.seek(0, VfsSeekWhence::Cur)? != 1 {
            return Err(VfsError::Io);
        }

        let cancelled = handle.prepare_read(2)?.acquire()?;
        if cancelled.bytes() != b"bc" {
            return Err(VfsError::Io);
        }
        drop(cancelled);
        if handle.seek(0, VfsSeekWhence::Cur)? != 1 {
            return Err(VfsError::Io);
        }

        let faulted = handle.prepare_read(2)?.acquire()?;
        if faulted.finish(VfsCopyProgress { copied : 0,
                                            complete : false })? !=
           VfsReadFinish::Fault ||
           handle.seek(0, VfsSeekWhence::Cur)? != 1
        {
            return Err(VfsError::Io);
        }

        let complete = handle.prepare_read(8)?.acquire()?;
        if complete.finish(VfsCopyProgress { copied : 3,
                                             complete : true })? !=
           VfsReadFinish::Bytes(3) ||
           handle.seek(0, VfsSeekWhence::Cur)? != 4
        {
            return Err(VfsError::Io);
        }
        let eof = handle.prepare_read(1)?.acquire()?;
        if !eof.bytes().is_empty() ||
           eof.finish(VfsCopyProgress { copied : 0,
                                        complete : true })? !=
           VfsReadFinish::Bytes(0)
        {
            return Err(VfsError::Io);
        }
        Ok(())
    }

    pub fn run() {
        #[cfg(feature = "bridge-fs-api")]
        {
            const NAME: &str = "vfs_rw_smoke";
            const DATA: &[u8] = b"vfs-smoke";
            if let Err(e) = rw_write_root_verify(VfsFsKind::Ext4, NAME, DATA) {
                log::warn!("[vfs] self_test rw verify skipped or failed: {:?}", e);
            }
            if let Err(e) = open_read_seek_smoke() {
                log::warn!("[vfs] self_test open/seek skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test open/seek ok");
            }
            if let Err(e) = read_at_write_at_smoke() {
                log::warn!("[vfs] self_test read_at/write_at skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test read_at/write_at ok");
            }
            if let Err(e) = open_description_sharing_smoke() {
                log::warn!("[vfs] self_test OFD sharing skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test OFD sharing ok");
            }
            if let Err(e) = prepared_read_smoke() {
                log::warn!("[vfs] self_test prepared read skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test prepared read ok");
            }
            const MKDIR_NAME: &str = "vfs_mkdir_smoke";
            if let Err(e) = rw_mkdir_verify(VfsFsKind::Ext4, MKDIR_NAME) {
                log::warn!("[vfs] self_test mkdir skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test mkdir ok");
            }
            if let Err(e) = null_dev_smoke() {
                log::warn!("[vfs] self_test /dev/null skipped or failed: {:?}", e);
            } else {
                log::info!("[vfs] self_test /dev/null ok");
            }
        }
        let _ = active_impl::backend().list_dev_nodes();
    }
