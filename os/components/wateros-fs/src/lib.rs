#![no_std]

//! 文件系统聚合层：统一 [`api_v0::FsImpl`] 注册、启动期根卷探测与挂载，并转发 devfs / rootfs 子 crate。
//!
//! 语义契约：[`init`] 在成功探测块设备后注入 rootfs 所选 impl 并尝试挂载；[`test`] 依赖特性与当前挂载状态，失败时仅记录日志不 panic。
extern crate alloc;

use alloc::vec::Vec;

/// 重导出 [`api_v0`]，供依赖方以 `wateros_fs::api` 访问稳定 API 面。
pub mod api {
    pub use ::api_v0::*;
}
/// 设备文件系统（devfs）子系统：节点枚举、块设备查找与默认根块路径。
pub mod devfs {
    pub use ::devfs::*;
}
/// 根文件系统（rootfs）子系统：当前根卷句柄与挂载入口。
pub mod rootfs {
    pub use ::rootfs::*;
}

#[cfg(feature = "impl-ext4")]
/// 可选的 ext4 实现 crate（由 `impl-ext4` feature 启用）。
pub use impl_ext4;

pub use api_v0::*;

/// 聚合层维护的 FS impl 注册表。每个 impl 暴露一个 `'static FsImpl`，按特性宏静态拼接。
///
/// 内核 devfs 是注册项之一（仅供 supported_fs 列示），ext4 是块设备路径上的真实根 FS impl。
pub fn registered_fs_impls() -> &'static [&'static dyn api_v0::FsImpl] {
    static TABLE: &[&'static dyn api_v0::FsImpl] = &[
        #[cfg(feature = "impl-ext4")]
        &impl_ext4::IMPL,
        &devfs::active_impl::IMPL,
    ];
    TABLE
}

/// 扁平化所有已注册 impl 的 [`FsCapability`]，便于 supported_fs 一行打印。
pub fn supported_fs_summary() -> Vec<api_v0::FsCapability> {
    let mut out = Vec::new();
    for imp in registered_fs_impls() {
        for cap in imp.supported() {
            out.push(*cap);
        }
    }
    out
}

/// 在注册表中选择一条匹配 `(kind, mode)` 的 impl；无匹配返回 None。
pub fn pick_fs_impl(
    kind: api_v0::FsKind,
    mode: api_v0::FsAccessMode,
) -> Option<&'static dyn api_v0::FsImpl> {
    registered_fs_impls()
        .iter()
        .copied()
        .find(|imp| imp.supports(kind, mode))
}

// 启动期诊断：把各 impl 声明的能力打到日志，便于对照 probe/mount 失败原因。
fn log_supported_fs() {
    for cap in supported_fs_summary() {
        logging::info!(
            "[fs] supported: kind={:?} access={:?}",
            cap.kind,
            cap.access
        );
    }
}

/// 文件系统子系统初始化：打印已注册能力、刷新 devfs，按默认根块路径探测并挂载只读根卷。
///
/// 当前行为：若无块设备、查找失败或无任何 impl 识别该卷，则提前返回并打日志。后续可扩展为多设备策略或用户态挂载协议。
pub fn init() {
    logging::info!("[fs] init begin");
    log_supported_fs();
    let node_count = devfs::active_impl::refresh();
    logging::info!("[fs] devfs refreshed, nodes={}", node_count);

    let Some(dev_path) = devfs::active_impl::default_root_block_path() else {
        logging::warn!("[fs] init: no root block device available");
        return;
    };
    let device = match devfs::active_impl::lookup_block_device(dev_path.as_str()) {
        Ok(d) => d,
        Err(err) => {
            logging::warn!(
                "[fs] init: lookup block device {:?} failed: {:?}",
                dev_path,
                err
            );
            return;
        }
    };

    // 边界：探测顺序即注册表顺序，首个同时通过 probe 且支持 RO 的 impl 被选中；无优先级 API。
    let mut chosen: Option<(&'static dyn api_v0::FsImpl, api_v0::FsKind)> = None;
    for imp in registered_fs_impls() {
        match imp.probe(&device) {
            Ok(Some(kind)) if imp.supports(kind, api_v0::FsAccessMode::ReadOnly) => {
                chosen = Some((*imp, kind));
                break;
            }
            Ok(_) => {}
            Err(err) => logging::warn!(
                "[fs] probe via {} failed: {:?}",
                imp.name(),
                err
            ),
        }
    }

    let Some((imp, kind)) = chosen else {
        logging::warn!(
            "[fs] init: no impl recognizes block device {:?}",
            dev_path
        );
        return;
    };
    logging::info!(
        "[fs] init: probe matched impl={} kind={:?}",
        imp.name(),
        kind
    );
    rootfs::active_impl::set_active_fs_impl(imp);

    match rootfs::active_impl::mount_default_root() {
        Ok(()) => {
            logging::info!("[fs] root fs mounted");
            logging::info!("[fs::boot-tree] /");
            for node in devfs::active_impl::list_nodes() {
                logging::info!(
                    "[fs::boot-tree] {} type={:?}",
                    node.path,
                    node.node_type
                );
            }
            if let Some(root) = rootfs::active_impl::root_fs() {
                root.lock().boot_dump_all_paths();
            }
        }
        Err(err) => logging::warn!("[fs] init failed: {:?}", err),
    }
}

/// 自检入口：调用 API 层样例测试；在启用 `impl-ext4` 时对已挂载 ext4 做最小 RO 校验（失败仅 warn）。
pub fn test() {
    logging::trace!("[fs] test begin");
    api_v0::test();

    #[cfg(feature = "impl-ext4")]
    {
        let Some(fs) = rootfs::active_impl::root_fs() else {
            logging::warn!(
                "[fs] ext4 ro test skipped: {:?}",
                api_v0::FsError::NotMounted
            );
            logging::trace!("[fs] test end");
            return;
        };
        if let Err(err) = impl_ext4::ro_self_test(fs) {
            logging::warn!("[fs] ext4 ro test failed: {:?}", err);
        }
    }
    logging::trace!("[fs] test end");
}
