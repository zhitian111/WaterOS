//! 根卷挂载代次：供 VFS、页缓存等依赖者识别卷实例已更换。

use core::sync::atomic::{AtomicU64, Ordering};

/// 每次成功发布新根卷或独立卷后递增；零表示尚未发生成功挂载。
static MOUNT_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 读取当前挂载代次；Acquire 与发布方的 Release 配对，保证新状态先于新代次可见。
pub fn mount_generation() -> u64 {
    MOUNT_GENERATION.load(Ordering::Acquire)
}

/// 递增挂载代次以使旧缓存键失效；极端回绕沿用 `u64` 环绕语义。
pub fn bump_mount_generation() {
    MOUNT_GENERATION.fetch_add(1, Ordering::Release);
}

pub(crate) fn next_mount_generation() {
    bump_mount_generation();
}
