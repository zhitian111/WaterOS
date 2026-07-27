//! 路径解析（相对 cwd → 绝对路径及符号链接展开）。

extern crate alloc;

use alloc::{string::String, vec::Vec};

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

/// 是否跟随路径的最终符号链接。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalSymlink {
    Follow,
    NoFollow,
}

/// 展开绝对路径中的符号链接。
///
/// `read_link` 在节点不是符号链接时返回 `Ok(None)`；`is_directory`
/// 用于验证仍有后续分量时当前节点确实是目录。
pub fn resolve_symlink_path_with<ReadLink, IsDirectory>(
    path: &str,
    final_symlink: FinalSymlink,
    mut read_link: ReadLink,
    mut is_directory: IsDirectory,
) -> VfsResult<String>
where
    ReadLink: FnMut(&str) -> VfsResult<Option<String>>,
    IsDirectory: FnMut(&str) -> VfsResult<bool>,
{
    const MAX_SYMLINKS: usize = 40;

    let normalized = normalize_absolute_path(path)?;
    let mut pending: Vec<String> = normalized
        .as_str()
        .split('/')
        .filter(|part| !part.is_empty())
        .map(String::from)
        .collect();
    let mut resolved: Vec<String> = Vec::new();
    let mut followed = 0usize;

    while !pending.is_empty() {
        let component = pending.remove(0);
        let mut candidate = String::from("/");
        if !resolved.is_empty() {
            candidate.push_str(resolved.join("/").as_str());
            candidate.push('/');
        }
        candidate.push_str(component.as_str());

        let is_final = pending.is_empty();
        let follow = !is_final || final_symlink == FinalSymlink::Follow;
        if follow {
            if let Some(target) = read_link(candidate.as_str())? {
                if followed == MAX_SYMLINKS {
                    return Err(VfsError::TooManySymlinks);
                }
                followed += 1;

                let parent = if resolved.is_empty() {
                    String::from("/")
                } else {
                    alloc::format!("/{}", resolved.join("/"))
                };
                let target = resolve_against_cwd(parent.as_str(), Some(target.as_str()))?;
                let mut combined = target;
                if !pending.is_empty() {
                    combined.push('/');
                    combined.push_str(pending.join("/").as_str());
                }
                let combined = normalize_absolute_path(combined.as_str())?;
                pending = combined
                    .as_str()
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .map(String::from)
                    .collect();
                resolved.clear();
                continue;
            }
        }

        if !is_final && !is_directory(candidate.as_str())? {
            return Err(VfsError::NotDirectory);
        }
        resolved.push(component);
    }

    if resolved.is_empty() {
        Ok(String::from("/"))
    } else {
        Ok(alloc::format!("/{}", resolved.join("/")))
    }
}
