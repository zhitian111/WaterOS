//! 诊断锁包装与 GDB 可读的锁引用。

use core::ops::{Deref, DerefMut};

use super::{lock_acquired, lock_released, lock_wait, DebugLockKind, ENABLED, NO_TASK};

/// 为关键的 `spin::Mutex` 添加诊断而不改变调用处的 `lock()` 形状。
///
/// `current_cpu` 只在 `enabled` 构建调用；普通 release 会被常量折叠为一次原始
/// `Mutex::lock()`。字段析构顺序保证先真正解锁，再从诊断区删除 owner。
pub struct TrackedMutex<T> {
    inner : spin::Mutex<T>,
    kind : DebugLockKind,
    current_cpu : fn() -> usize,
}
impl<T> TrackedMutex<T> {
    pub const fn new(value : T, kind : DebugLockKind, current_cpu : fn() -> usize) -> Self {
        Self { inner : spin::Mutex::new(value), kind, current_cpu }
    }
}

impl<T> TrackedMutex<T> {
    #[inline]
    pub fn lock(&self) -> TrackedMutexGuard<'_, T> {
        if !ENABLED {
            return TrackedMutexGuard { guard : self.inner.lock(), _scope : None };
        }
        let cpu = (self.current_cpu)();
        let object = self as *const _ as *const () as usize;
        let guard = if let Some(guard) = self.inner.try_lock() {
            guard
        } else {
            lock_wait(cpu, 0, NO_TASK, self.kind, object);
            self.inner.lock()
        };
        lock_acquired(cpu, self.kind, object);
        TrackedMutexGuard { guard,
                            _scope : Some(DebugLockScope { cpu,
                                                           kind : self.kind,
                                                           object }) }
    }

    /// 不阻塞地尝试获取锁。失败不会重复记录 contention；需要轮询的调用方可
    /// 在第一次失败时显式调用 [`lock_wait`]。
    #[inline]
    pub fn try_lock(&self) -> Option<TrackedMutexGuard<'_, T>> {
        let guard = self.inner.try_lock()?;
        if !ENABLED {
            return Some(TrackedMutexGuard { guard, _scope : None });
        }
        let cpu = (self.current_cpu)();
        let object = self as *const _ as *const () as usize;
        lock_acquired(cpu, self.kind, object);
        Some(TrackedMutexGuard { guard,
                                 _scope : Some(DebugLockScope { cpu,
                                                                kind : self.kind,
                                                                object }) })
    }

    pub fn debug_identity(&self) -> (DebugLockKind, usize) {
        (self.kind, self as *const _ as *const () as usize)
    }
}

struct DebugLockScope {
    cpu : usize,
    kind : DebugLockKind,
    object : usize,
}

impl Drop for DebugLockScope {
    fn drop(&mut self) { lock_released(self.cpu, self.kind, self.object); }
}

pub struct TrackedMutexGuard<'a, T : ?Sized> {
    // Rust 按声明顺序析构字段：先释放真实 mutex，再发布诊断 release。
    guard : spin::MutexGuard<'a, T>,
    _scope : Option<DebugLockScope>,
}

impl<T : ?Sized> Deref for TrackedMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.guard }
}

impl<T : ?Sized> DerefMut for TrackedMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.guard }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DebugLockRef {
    pub kind : u16,
    pub _reserved : [u16; 3],
    pub object : u64,
}
