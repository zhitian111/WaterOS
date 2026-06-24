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
        ensure_dir(sess.as_mut(), "/usr", 0o755);
        ensure_dir(sess.as_mut(), "/usr/bin", 0o755);
        ensure_dir(sess.as_mut(), "/sbin", 0o755);
        ensure_dir(sess.as_mut(), "/usr/sbin", 0o755);
        ensure_dir(sess.as_mut(), "/etc", 0o755);
        ensure_etc_passwd(sess.as_mut());

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

        match sess.mkdir("/var", 0o755) {
            Ok(()) => info!("[{LOG_TAG}] mkdir /var ok"),
            Err(VfsError::Exists) => trace!("[{LOG_TAG}] /var already present"),
            Err(e) => {
                warn!("[{LOG_TAG}] mkdir /var failed: {e:?}");
                return;
            }
        }

        match sess.mkdir("/var/tmp", 0o777) {
            Ok(()) => info!("[{LOG_TAG}] mkdir /var/tmp ok"),
            Err(VfsError::Exists) => trace!("[{LOG_TAG}] /var/tmp already present"),
            Err(e) => {
                warn!("[{LOG_TAG}] mkdir /var/tmp failed: {e:?}");
                return;
            }
        }

        /// busybox 多用途小程序：优先给 libc 本地目录建链接，避免 musl/glibc 脚本
        /// 通过 PATH 误用另一套动态库；同时保留 /bin 兼容路径。
        const APPLETS : &[&str] = &["ls", "sleep", "basename", "cp"];
        for applet in APPLETS {
            try_hardlink(sess.as_mut(),
                         "/glibc/busybox",
                         alloc::format!("/glibc/{applet}").as_str());
            try_hardlink(sess.as_mut(),
                         "/musl/busybox",
                         alloc::format!("/musl/{applet}").as_str());
            try_hardlink(sess.as_mut(),
                         "/glibc/busybox",
                         alloc::format!("/bin/{applet}").as_str());
            try_hardlink(sess.as_mut(),
                         "/glibc/busybox",
                         alloc::format!("/usr/bin/{applet}").as_str());
        }
    }
}

#[cfg(feature = "vfs-bridge")]
fn ensure_etc_passwd(sess : &mut (impl vfs::api::RootRwSession + ?Sized)) {
    const PASSWD : &str = "root:x:0:0:root:/root:/bin/sh\n\
nobody:x:65534:65534:nobody:/:/bin/false\n";
    const GROUP : &str = "root:x:0:\n\
nogroup:x:65534:\n";

    write_text_file(sess, "/etc/passwd", PASSWD.as_bytes());
    write_text_file(sess, "/etc/group", GROUP.as_bytes());
}

#[cfg(feature = "vfs-bridge")]
fn write_text_file(sess : &mut (impl vfs::api::RootRwSession + ?Sized), path : &str, data : &[u8]) {
    use vfs::api::VfsError;

    let _ = sess.unlink(path);
    match sess.write_regular_file(path, data) {
        Ok(()) => info!("[{LOG_TAG}] wrote {path} ({} bytes)", data.len()),
        Err(e) => warn!("[{LOG_TAG}] write {path} failed: {e:?}"),
    }
}

#[cfg(feature = "vfs-bridge")]
fn ensure_dir(sess : &mut (impl vfs::api::RootRwSession + ?Sized), path : &str, mode : u32) {
    use vfs::api::VfsError;

    match sess.mkdir(path, mode) {
        Ok(()) => info!("[{LOG_TAG}] mkdir {path} ok"),
        Err(VfsError::Exists) => trace!("[{LOG_TAG}] {path} already present"),
        Err(e) => warn!("[{LOG_TAG}] mkdir {path} failed: {e:?}"),
    }
}

#[cfg(feature = "vfs-bridge")]
fn try_hardlink(sess : &mut (impl vfs::api::RootRwSession + ?Sized), src : &str, dest : &str) {
    use vfs::api::VfsError;

    match sess.hardlink(src, dest) {
        Ok(()) => info!("[{LOG_TAG}] hardlink {dest} -> {src}"),
        Err(VfsError::Exists) => trace!("[{LOG_TAG}] {dest} already present"),
        Err(VfsError::NotFound) => trace!("[{LOG_TAG}] hardlink {dest} skipped: {src} missing"),
        Err(e) => warn!("[{LOG_TAG}] hardlink {dest} -> {src} failed: {e:?}"),
    }
}
