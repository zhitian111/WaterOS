//! 绝对路径规范化：用于 VFS 入口在委托 FS 前统一路径形状（不访问块设备）。

use alloc::string::String;

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
    let mut out = String::with_capacity(path.len());
    out.push('/');
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if out.len() > 1 {
                let prefix = &out[..out.len() - 1];
                if let Some(pos) = prefix.rfind('/') {
                    out.truncate(pos + 1);
                } else {
                    out.truncate(1);
                }
                if out.len() > 1 && out.ends_with('/') {
                    out.pop();
                }
            }
            continue;
        }
        if out.len() > 1 && !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(part);
    }
    Ok(NormalizedPath { inner: out })
}

/// 根目录下文件名不得包含 `/` 或为空。
pub fn validate_root_file_name(name: &str) -> VfsResult<()> {
    if name.is_empty() || name.contains('/') {
        return Err(VfsError::InvalidPath);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_absolute_path;

    #[test]
    fn removes_dot_and_redundant_slashes() {
        assert_eq!(normalize_absolute_path("//a//b/").unwrap().as_str(), "/a/b");
    }

    #[test]
    fn resolves_parent_segments_without_escaping_root() {
        assert_eq!(normalize_absolute_path("/a/./b/../c").unwrap().as_str(), "/a/c");
        assert_eq!(normalize_absolute_path("/a/..").unwrap().as_str(), "/");
        assert_eq!(normalize_absolute_path("/a/../../").unwrap().as_str(), "/");
    }

    #[test]
    fn preserves_utf8_components() {
        assert_eq!(normalize_absolute_path("/tmp/测试/ok").unwrap().as_str(), "/tmp/测试/ok");
    }
}
