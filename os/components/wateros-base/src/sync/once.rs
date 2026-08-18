//! 一次初始化、初始化后无锁读取的全局容器。

use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};

const UNINITIALIZED : u8 = 0;
const INITIALIZING : u8 = 1;
const INITIALIZED : u8 = 2;

/// 一次初始化失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnceInitError {
    /// 该容器已经完成初始化，或另一个 CPU 正在初始化它。
    AlreadyInitialized,
}

/// `BootOnceCell` 和 `RuntimeOnceCell` 共用的发布状态机。
///
/// ONCE_PUBLISH: 初始化者先写 `value`，再以 `Release` 发布 `INITIALIZED`；
/// 读取者只有在 `Acquire` 观察到该状态后才读取 `value`。
struct OnceStorage<T> {
    state : AtomicU8,
    value : UnsafeCell<MaybeUninit<T>>,
}

// ONCE_PUBLISH: 初始化过程需要把 T 从初始化 CPU 移交给其他 CPU（Send），
// 发布后的共享引用又要求 T: Sync。状态机保证 value 只写一次。
unsafe impl<T : Send + Sync> Sync for OnceStorage<T> {}

impl<T> OnceStorage<T> {
    const fn new() -> Self {
        Self { state : AtomicU8::new(UNINITIALIZED),
               value : UnsafeCell::new(MaybeUninit::uninit()) }
    }

    fn is_initialized(&self) -> bool {
        self.state
            .load(Ordering::Acquire) ==
        INITIALIZED
    }

    fn get(&self) -> Option<&T> {
        if self.is_initialized() {
            // Acquire 读取与初始化者的 Release 配对，确保下面的引用看到完整的 T。
            Some(unsafe { (&*self.value.get()).assume_init_ref() })
        } else {
            None
        }
    }

    fn init(&self, value : T) -> Result<(), OnceInitError> {
        match self.state
                  .compare_exchange(UNINITIALIZED,
                                    INITIALIZING,
                                    Ordering::Relaxed,
                                    Ordering::Acquire)
        {
            Ok(_) => {}
            Err(INITIALIZING) => {
                // 初始化中的槽位不能被读取或覆盖；等待发布完成后统一返回错误。
                while self.state
                          .load(Ordering::Acquire) ==
                      INITIALIZING
                {
                    spin_loop();
                }
                return Err(OnceInitError::AlreadyInitialized);
            }
            Err(_) => return Err(OnceInitError::AlreadyInitialized),
        }

        // 只有成功抢到 INITIALIZING 状态的 CPU 能写入槽位，因此不会并发覆盖 T。
        unsafe { (*self.value.get()).write(value) };
        self.state
            .store(INITIALIZED, Ordering::Release);
        Ok(())
    }
}

impl<T> Drop for OnceStorage<T> {
    fn drop(&mut self) {
        if *self.state.get_mut() == INITIALIZED {
            unsafe {
                self.value
                    .get_mut()
                    .assume_init_drop()
            };
        }
    }
}

/// 仅允许在 BSP boot 阶段写入一次，之后无锁只读的容器。
///
/// 与 [`RuntimeOnceCell`] 使用同一套线程安全发布机制；独立类型用于在接口层表达
/// 生命周期约束：该值应在开放 AP 或运行时消费者前完成初始化。
pub struct BootOnceCell<T> {
    inner : OnceStorage<T>,
}

impl<T> BootOnceCell<T> {
    /// 构造一个尚未初始化的 boot cell。
    pub const fn new() -> Self { Self { inner : OnceStorage::new() } }

    /// 在 boot 阶段初始化。
    ///
    /// BOOT_CONTRACT: 应在开放依赖该值的 AP/运行时路径前调用。容器自身仍通过
    /// 原子状态机保证并发调用不会造成数据竞争；重复初始化返回错误。
    pub fn init(&self, value : T) -> Result<(), OnceInitError> {
        self.inner
            .init(value)
    }

    /// 判断值是否已经完成发布。
    pub fn is_initialized(&self) -> bool {
        self.inner
            .is_initialized()
    }

    /// 返回已发布值；读取路径不获取锁。
    pub fn get(&self) -> Option<&T> { self.inner.get() }
}

impl<T> Default for BootOnceCell<T> {
    fn default() -> Self { Self::new() }
}

/// 可在多核运行期安全竞争初始化，成功后无锁只读的容器。
pub struct RuntimeOnceCell<T> {
    inner : OnceStorage<T>,
}

impl<T> RuntimeOnceCell<T> {
    /// 构造一个尚未初始化的 runtime cell。
    pub const fn new() -> Self { Self { inner : OnceStorage::new() } }

    /// 竞争初始化；至多一个调用成功，其他调用等待该值完成发布后返回错误。
    ///
    /// 当前内核使用 abort/panic-stop 语义；初始化闭包不存在，因此不会留下需要
    /// 恢复的“初始化中”状态。
    pub fn init(&self, value : T) -> Result<(), OnceInitError> {
        self.inner
            .init(value)
    }

    /// 判断值是否已经完成发布。
    pub fn is_initialized(&self) -> bool {
        self.inner
            .is_initialized()
    }

    /// 返回已发布值；读取路径不获取锁。
    pub fn get(&self) -> Option<&T> { self.inner.get() }
}

impl<T> Default for RuntimeOnceCell<T> {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use self::std::sync::Arc;
    use self::std::thread;
    use super::{BootOnceCell, OnceInitError, RuntimeOnceCell};

    #[test]
    fn boot_cell_is_read_only_after_init() {
        let cell = BootOnceCell::new();
        assert!(!cell.is_initialized());
        assert!(cell.get().is_none());
        cell.init(42)
            .unwrap();
        assert!(cell.is_initialized());
        assert_eq!(cell.get(), Some(&42));
        assert_eq!(cell.init(7),
                   Err(OnceInitError::AlreadyInitialized));
    }

    #[test]
    fn runtime_cell_initializes_once() {
        let cell = RuntimeOnceCell::new();
        cell.init(42)
            .unwrap();
        assert_eq!(cell.init(7),
                   Err(OnceInitError::AlreadyInitialized));
        assert_eq!(cell.get(), Some(&42));
    }

    #[test]
    fn runtime_cell_publishes_one_competing_value() {
        let cell = Arc::new(RuntimeOnceCell::new());
        let threads : self::std::vec::Vec<_> = (0..8).map(|value| {
                                                         let cell = Arc::clone(&cell);
                                                         thread::spawn(move || cell.init(value))
                                                     })
                                                     .collect();
        let successes = threads.into_iter()
                               .map(|thread| {
                                   thread.join()
                                         .unwrap()
                               })
                               .filter(Result::is_ok)
                               .count();
        assert_eq!(successes, 1);
        assert!(cell.is_initialized());
        assert!(cell.get().is_some());
    }
}
