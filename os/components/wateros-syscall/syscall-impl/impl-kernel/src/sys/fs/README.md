# fs syscall

本目录实现路径解析、fd 生命周期、普通文件 I/O 和文件事件。路径由 `path_at.rs`
统一解析，打开文件描述由 VFS 管理，handler 不长期持有 fd registry 锁。

## 当前能力

- `openat/openat2`、close/close_range、dup/dup3、fcntl/flock、cwd 与 `*at` 路径。
- read/write、向量与定位 I/O、lseek、stat/statx/statfs、目录枚举和属性/xattr。
- pipe2、sendfile、splice/tee/vmsplice、copy_file_range。
- truncate/fallocate 的后端可实现子集、sync/fsync/fdatasync。
- `memfd_create`：共享 mmap、truncate、CLOEXEC 与 seals。
- `inotify`：create/modify/attrib/move/delete/self/ignored、cookie、poll 和读取回滚。

## 已知边界

- `openat2 RESOLVE_IN_ROOT` 尚未安全实现；`RESOLVE_CACHED` 因无纯 dcache 路径返回
  `EAGAIN`。
- inotify 事件目前由 syscall/VFS 变更入口发布；新增内核内部写路径时必须接入统一
  VFS mutation hook，之后再补 access/close/writeback 事件。
- ext4 后端不支持的预分配、打洞或部分 xattr 会返回 `EOPNOTSUPP`，不会假成功。

## 扩展方向

建立 per-process root 后再实现 chroot/pivot_root；把路径约束下沉到统一 resolver，
并补 fanotify、file handle、异步预读与 page-cache writeback 统计。
