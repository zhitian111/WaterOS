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
        ensure_dir(sess.as_mut(), "/boot", 0o755);
        ensure_dir(sess.as_mut(), "/root", 0o700);
        ensure_etc_passwd(sess.as_mut());
        ensure_kernel_config(sess.as_mut());

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
        match vfs::mount_bootstrap_tmpfs_at("/tmp") {
            Ok(()) => info!("[{LOG_TAG}] mounted tmpfs at /tmp"),
            Err(VfsError::Exists) => trace!("[{LOG_TAG}] tmpfs already mounted at /tmp"),
            Err(e) => warn!("[{LOG_TAG}] mount tmpfs /tmp failed: {e:?}"),
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
        /// `test` 不链到 `/glibc`/`/musl` 根：赛题 busybox 用例会 `mv test_dir test` 建目录。
        const APPLETS : &[&str] =
            &["sh", "ls", "sleep", "basename", "cp", "mkdir", "rmdir", "cat", "grep", "awk", "cut",
              "sed", "tr", "wc", "head", "tail", "sort", "uniq", "expr", "dirname", "readlink",
              "ln", "rm", "touch", "chmod", "chown", "mktemp", "printf", "test", "true", "false",
              "pwd", "env", "which", "id", "whoami", "groups", "date", "uname", "dd", "od",
              "hexdump", "xargs", "find", "cmp", "diff", "seq", "tee", "tac", "kill", "mount",
              "umount", "ip", "ifconfig", "route", "sysctl", "arping"];
        const SKIP_LIBC_ROOT_APPLETS : &[&str] = &["test"];
        for applet in APPLETS {
            if !SKIP_LIBC_ROOT_APPLETS.contains(&applet) {
                try_hardlink(sess.as_mut(),
                             "/glibc/busybox",
                             alloc::format!("/glibc/{applet}").as_str());
                try_hardlink(sess.as_mut(),
                             "/musl/busybox",
                             alloc::format!("/musl/{applet}").as_str());
            }
            try_hardlink(sess.as_mut(),
                         "/glibc/busybox",
                         alloc::format!("/bin/{applet}").as_str());
            try_hardlink(sess.as_mut(),
                         "/glibc/busybox",
                         alloc::format!("/sbin/{applet}").as_str());
            try_hardlink(sess.as_mut(),
                         "/glibc/busybox",
                         alloc::format!("/usr/sbin/{applet}").as_str());
        }

        const UNSUPPORTED_APPLETS : &[&str] = &["locale", "ar", "rsh"];
        for applet in UNSUPPORTED_APPLETS {
            remove_applet_links(sess.as_mut(), applet);
        }
    }
}

#[cfg(feature = "vfs-bridge")]
fn ensure_etc_passwd(sess : &mut (impl vfs::api::RootRwSession + ?Sized)) {
    use vfs::api::SingleRootReadView;

    const PASSWD : &str = "root:x:0:0:root:/root:/bin/sh\ndaemon:x:1:1:daemon:/usr/sbin:/bin/\
                           false\nnobody:x:65534:65534:nobody:/nonexistent:/bin/false\n";
    const GROUP : &str = "root:x:0:\ndaemon:x:1:\nnobody:x:65534:\nnogroup:x:65534:\n";
    const NSSWITCH : &str =
        "passwd: files\ngroup: files\nshadow: files\ngshadow: files\nhosts: files dns\n";
    const PROTOCOLS : &str = "ip 0 IP\nhopopt 0 HOPOPT\nicmp 1 ICMP\nigmp 2 IGMP\nggp 3 GGP\ntcp \
                              6 TCP\nudp 17 UDP\nipv6 41 IPv6\nipv6-route 43 \
                              IPv6-Route\nipv6-frag 44 IPv6-Frag\nesp 50 ESP\nah 51 AH\nipv6-icmp \
                              58 IPv6-ICMP\nipv6-nonxt 59 IPv6-NoNxt\nipv6-opts 60 IPv6-Opts\nraw \
                              255 RAW\n";

    for (path, data, mode) in [("/etc/passwd", PASSWD.as_bytes(), 0o644),
                               ("/etc/group", GROUP.as_bytes(), 0o644),
                               ("/etc/nsswitch.conf", NSSWITCH.as_bytes(), 0o644),
                               ("/etc/protocols", PROTOCOLS.as_bytes(), 0o644)]
    {
        match vfs::overwrite_absolute_file(path, data) {
            Ok(()) => info!("[{LOG_TAG}] overwrote {path} ({} bytes)",
                            data.len()),
            Err(e) => warn!("[{LOG_TAG}] overwrite {path} failed: {e:?}"),
        }
        let _ = sess.chmod(path, mode);
    }

    match vfs::root::read_view().read("/etc/passwd") {
        Ok(data)
            if data.windows(6)
                   .any(|w| w == b"nobody") =>
        {
            info!("[{LOG_TAG}] verified /etc/passwd contains nobody ({} bytes)",
                  data.len());
        }
        Ok(data) => {
            warn!("[{LOG_TAG}] /etc/passwd missing nobody after overwrite ({} bytes)",
                  data.len());
        }
        Err(e) => warn!("[{LOG_TAG}] read back /etc/passwd failed: {e:?}"),
    }
}

#[cfg(feature = "vfs-bridge")]
fn ensure_kernel_config(sess : &mut (impl vfs::api::RootRwSession + ?Sized)) {
    const CONFIG : &str = "\
# WaterOS kernel config exposed for LTP kconfig probes
CONFIG_BSD_PROCESS_ACCT=y
# CONFIG_BSD_PROCESS_ACCT_V3 is not set
CONFIG_EVENTFD=y
# CONFIG_KEYS is not set
# CONFIG_AF_ALG is not set
# CONFIG_AIO is not set
# CONFIG_EXT4_FS_POSIX_ACL is not set
";

    match vfs::overwrite_absolute_file("/boot/config-5.15.0", CONFIG.as_bytes()) {
        Ok(()) => info!("[{LOG_TAG}] overwrote /boot/config-5.15.0 ({} bytes)",
                        CONFIG.len()),
        Err(e) => warn!("[{LOG_TAG}] overwrite /boot/config-5.15.0 failed: {e:?}"),
    }
    let _ = sess.chmod("/boot/config-5.15.0", 0o644);
}

/// Bring-up：从 `/{glibc,musl}/ltp/testcases/bin/` 删除排除名单中的顶层用例文件。
///
/// `ltp_testcode.sh` 用 `for file in "$target_dir"/*` 顺序跑测；删文件比 exec 后 fast-exit
/// 更省时（不 fork/exec/wait），并避免在通用 syscall 路径中识别测试程序。
#[cfg(feature = "vfs-bridge")]
pub fn prune_ltp_excluded_testcases() {
    use vfs::api::{VfsError, VfsFsKind};

    let Ok(mut sess) = vfs::mount::open_rw_session(VfsFsKind::Ext4) else {
        warn!("[{LOG_TAG}] prune_ltp_excluded_testcases: open_rw_session failed");
        return;
    };

    let basenames = crate::user_bringup_ltp_exclusions::ltp_submit_skip_basenames();
    let mut removed = 0u32;
    let mut absent = 0u32;
    let mut failed = 0u32;

    for prefix in ["/glibc/ltp/testcases/bin",
                   "/musl/ltp/testcases/bin"]
    {
        for basename in basenames {
            let path = alloc::format!("{prefix}/{basename}");
            match sess.unlink(path.as_str()) {
                Ok(()) => removed += 1,
                Err(VfsError::NotFound) => absent += 1,
                Err(e) => {
                    failed += 1;
                    if failed <= 8 {
                        warn!("[{LOG_TAG}] prune LTP {path}: {e:?}");
                    }
                }
            }
        }
    }
    let musl_basenames = crate::user_bringup_ltp_exclusions::ltp_musl_skip_basenames();
    for basename in musl_basenames {
        let path = alloc::format!("/musl/ltp/testcases/bin/{basename}");
        match sess.unlink(path.as_str()) {
            Ok(()) => removed += 1,
            Err(VfsError::NotFound) => absent += 1,
            Err(e) => {
                failed += 1;
                if failed <= 8 {
                    warn!("[{LOG_TAG}] prune musl LTP {path}: {e:?}");
                }
            }
        }
    }
    info!("[{LOG_TAG}] prune_ltp_excluded: {} common basenames × 2 libc + {} musl-only, \
           removed={removed} absent={absent} failed={failed}",
          basenames.len(),
          musl_basenames.len());
}

#[cfg(not(feature = "vfs-bridge"))]
pub fn prune_ltp_excluded_testcases() {
    warn!("[{LOG_TAG}] vfs-bridge off: skip LTP testcase prune");
}

/// LTP 用例依赖的账户文件；在统一 `self_test` 之后再次写入，避免自检覆盖。
#[cfg(feature = "vfs-bridge")]
pub fn refresh_ltp_accounts() {
    use vfs::api::VfsFsKind;

    let Ok(mut sess) = vfs::mount::open_rw_session(VfsFsKind::Ext4) else {
        warn!("[{LOG_TAG}] refresh_ltp_accounts: open_rw_session failed");
        return;
    };
    ensure_etc_passwd(sess.as_mut());
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

#[cfg(feature = "vfs-bridge")]
fn remove_applet_links(sess : &mut (impl vfs::api::RootRwSession + ?Sized), applet : &str) {
    use vfs::api::VfsError;

    for path in [alloc::format!("/glibc/{applet}"),
                 alloc::format!("/musl/{applet}"),
                 alloc::format!("/bin/{applet}"),
                 alloc::format!("/usr/bin/{applet}"),
                 alloc::format!("/sbin/{applet}"),
                 alloc::format!("/usr/sbin/{applet}")]
    {
        match sess.unlink(path.as_str()) {
            Ok(()) => info!("[{LOG_TAG}] removed unsupported applet link {path}"),
            Err(VfsError::NotFound) => trace!("[{LOG_TAG}] unsupported applet link {path} absent"),
            Err(e) => warn!("[{LOG_TAG}] remove unsupported applet link {path} failed: {e:?}"),
        }
    }
}
