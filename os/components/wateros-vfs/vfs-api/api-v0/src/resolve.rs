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
