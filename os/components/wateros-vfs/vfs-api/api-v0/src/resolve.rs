//! 路径解析（相对 cwd → 绝对路径）。

extern crate alloc;

use alloc::string::String;

use crate::error::{VfsError, VfsResult};
use crate::path::normalize_absolute_path;

/// 将 `path` 相对 `cwd` 解析为规范化绝对路径。
pub fn resolve_against_cwd(cwd: &str, path: Option<&str>) -> VfsResult<String> {
    let Some(p) = path else {
        return Err(VfsError::InvalidPath);
    };
    let combined = if p.starts_with('/') {
        String::from(p)
    } else if cwd == "/" {
        alloc::format!("/{}", p.trim_start_matches('/'))
    } else {
        alloc::format!("{}/{}", cwd.trim_end_matches('/'), p.trim_start_matches('/'))
    };
    Ok(String::from(normalize_absolute_path(combined.as_str())?.as_str()))
}

/// 可选：`open` 前将用户路径解析为绝对路径（由聚合层注册，可含 per-task cwd）。
type OpenPathResolverFn = fn(&str) -> VfsResult<String>;

static OPEN_PATH_RESOLVER: spin::Mutex<Option<OpenPathResolverFn>> = spin::Mutex::new(None);

/// 注册 `open` 路径解析钩子（单核启动期调用一次即可）。
pub fn register_open_path_resolver(resolver: OpenPathResolverFn) {
    *OPEN_PATH_RESOLVER.lock() = Some(resolver);
}

/// 解析 `open`/`openat` 传入的路径：已注册则走 per-task cwd，否则相对 `/`。
pub fn resolve_open_path(path: &str) -> VfsResult<String> {
    if let Some(resolver) = *OPEN_PATH_RESOLVER.lock() {
        return resolver(path);
    }
    resolve_against_cwd("/", Some(path))
}
