//! 唯一 SHM registry 的全局入口。

use spin::Mutex;

use crate::registry::ShmRegistry;

/// `LOCK:` 全局段、key 和 task attachment 索引的唯一锁。
///
/// `SMP:` 仅保护 SHM 元数据和帧回收决定；调用方必须在释放该锁之后再执行 MM 映射、TLB
/// shootdown、调度或其他可能阻塞的操作。
static SHM_REGISTRY: Mutex<ShmRegistry> = Mutex::new(ShmRegistry::new());

/// 返回全局 SHM registry。保留该显式锁接口，是因为 `shmat` 需要先取得段快照、解锁后映射，
/// 成功才回到 registry 提交 attachment。
pub fn registry() -> &'static Mutex<ShmRegistry> {
    &SHM_REGISTRY
}
