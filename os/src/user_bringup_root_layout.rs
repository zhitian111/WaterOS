//! 启动期根卷布局：为 busybox PATH 探测补齐 `/bin/ls`、`/bin/basename` 等链接（不修改赛题镜像）。

use runtime::logging::*;

const LOG_TAG : &str = "root-layout";

/// 在 RW 根卷挂载后创建 busybox 与 libc 测例需要的基础目录/链接（幂等）。
pub fn ensure_busybox_path_links() {
    #[cfg(not(feature = "vfs-bridge"))]
    {
        warn!("[{LOG_TAG}] vfs-bridge off: skip /bin/ls layout");
        return;
    }

    #[cfg(feature = "vfs-bridge")]
    {
        use vfs::api::{VfsError, VfsFsKind};

        let mut sess = match vfs::mount::open_rw_session(VfsFsKind::Ext4) {
            Ok(s) => s,
            Err(e) => {
                warn!("[{LOG_TAG}] open_rw_session failed: {e:?}");
                return;
            }
        };

        if let Err(e) = sess.mkdir("/bin", 0o755) {
            if e != VfsError::Exists {
                warn!("[{LOG_TAG}] mkdir /bin failed: {e:?}");
                return;
            }
        }

        match sess.mkdir("/dev", 0o755) {
            Ok(()) => info!("[{LOG_TAG}] mkdir /dev ok"),
            Err(VfsError::Exists) => trace!("[{LOG_TAG}] /dev already present"),
            Err(e) => {
                warn!("[{LOG_TAG}] mkdir /dev failed: {e:?}");
                return;
            }
        }

        match sess.mkdir("/dev/shm", 0o1777) {
            Ok(()) => info!("[{LOG_TAG}] mkdir /dev/shm ok"),
            Err(VfsError::Exists) => trace!("[{LOG_TAG}] /dev/shm already present"),
            Err(e) => {
                warn!("[{LOG_TAG}] mkdir /dev/shm failed: {e:?}");
                return;
            }
        }

        match sess.mkdir("/tmp", 0o777) {
            Ok(()) => info!("[{LOG_TAG}] mkdir /tmp ok"),
            Err(VfsError::Exists) => info!("[{LOG_TAG}] /tmp already present"),
            Err(e) => {
                warn!("[{LOG_TAG}] mkdir /tmp failed: {e:?}");
                return;
            }
        }

        /// busybox 多用途小程序：与 `/glibc/busybox` 硬链接到 `/bin/<name>`，供 PATH 解析。
        const BUSYBOX_APPLET_LINKS: &[&str] = &["/bin/ls", "/bin/sleep", "/bin/basename"];

        for dest in BUSYBOX_APPLET_LINKS {
            try_hardlink_busybox_applet(sess.as_mut(), dest);
        }
    }
}

#[cfg(feature = "vfs-bridge")]
fn try_hardlink_busybox_applet(sess: &mut (impl vfs::api::RootRwSession + ?Sized), dest: &str) {
    use vfs::api::VfsError;

    match sess.hardlink("/glibc/busybox", dest) {
        Ok(()) => info!("[{LOG_TAG}] hardlink {dest} -> /glibc/busybox"),
        Err(VfsError::Exists) => trace!("[{LOG_TAG}] {dest} already present"),
        Err(e) => warn!("[{LOG_TAG}] hardlink {dest} failed: {e:?}"),
    }
}
