//! 绝对路径规范化：用于 VFS 入口在委托 FS 前统一路径形状（不访问块设备）。

use alloc::{string::String, vec::Vec};

use crate::{VfsError, VfsResult};

/// 规范化后的绝对路径，保证以 `/` 开头、无 `//`、无 `.` / 已解析的 `..`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPath {
    inner: String,
}

impl NormalizedPath {
    /// 以 `str` 形式借用规范化结果，可直接传给后端 FS（已保证以 `/` 开头等不变量）。
    pub fn as_str(&self) -> &str { self.inner.as_str() }
}

/// 将用户输入路径规范化为根卷查找用的绝对路径。
///
/// - 必须以 `/` 开头，否则 [`VfsError::InvalidPath`]。
/// - 空路径（`""`）非法。
/// - `..` 在根之上折叠为根（与常见 Unix 行为一致）。
pub fn normalize_absolute_path(path: &str) -> VfsResult<NormalizedPath> {
    if path.is_empty() {
        return Err(VfsError::InvalidPath);
    }
    if !path.starts_with('/') {
        return Err(VfsError::InvalidPath);
    }
    let mut stack: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            let _ = stack.pop();
            continue;
        }
        stack.push(part);
    }
    let mut out = String::with_capacity(path.len());
    out.push('/');
    for (i, p) in stack.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(p);
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    Ok(NormalizedPath { inner: out })
}
