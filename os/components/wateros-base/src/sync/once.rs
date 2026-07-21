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
    AlreadyInitialized,
}

struct OnceStorage<T> {
    state : AtomicU8,
    value : UnsafeCell<MaybeUninit<T>>,
}

unsafe impl<T : Send + Sync> Sync for OnceStorage<T> {}

impl<T> OnceStorage<T> {
    const fn new() -> Self {
        Self { state : AtomicU8::new(UNINITIALIZED),
               value : UnsafeCell::new(MaybeUninit::uninit()) }
    }

    fn check(&self) -> bool {
        self.state
            .load(Ordering::Acquire) ==
        INITIALIZED
    }

    fn get(&self) -> Option<&T> {
        if self.check() {
            // Acquire above observes the value write published by init's Release store.
            Some(unsafe { (&*self.value.get()).assume_init_ref() })
        } else {
            None
        }
    }

    fn init(&self, value : T) -> Result<(), OnceInitError> {
        match self.state
                  .compare_exchange(UNINITIALIZED,
                                    INITIALIZING,
                                    Ordering::Acquire,
                                    Ordering::Acquire)
        {
            Ok(_) => {}
            Err(INITIALIZING) => {
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
pub struct BootOnceCell<T> {
    inner : OnceStorage<T>,
}

impl<T> BootOnceCell<T> {
    pub const fn new() -> Self { Self { inner : OnceStorage::new() } }

    /// 在 boot 阶段初始化。
    ///
    /// # Safety
    /// 调用方必须保证内核仍在受控 boot 阶段，尚未把依赖该值的运行时路径开放给 AP。
    pub unsafe fn init(&self, value : T) -> Result<(), OnceInitError> {
        self.inner
            .init(value)
    }

    pub fn check(&self) -> bool { self.inner.check() }

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
    pub const fn new() -> Self { Self { inner : OnceStorage::new() } }

    /// 竞争初始化；至多一个调用成功，其他调用等待发布完成后返回错误。
    pub fn init(&self, value : T) -> Result<(), OnceInitError> {
        self.inner
            .init(value)
    }

    pub fn check(&self) -> bool { self.inner.check() }

    /// 返回已发布值；读取路径不获取锁。
    pub fn get(&self) -> Option<&T> { self.inner.get() }
}

impl<T> Default for RuntimeOnceCell<T> {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{BootOnceCell, OnceInitError, RuntimeOnceCell};
    use self::std::sync::Arc;
    use self::std::thread;

    #[test]
    fn boot_cell_is_read_only_after_init() {
        let cell = BootOnceCell::new();
        assert!(!cell.check());
        assert!(cell.get().is_none());
        unsafe {
            cell.init(42)
                .unwrap()
        };
        assert!(cell.check());
        assert_eq!(cell.get(), Some(&42));
        assert_eq!(unsafe { cell.init(7) },
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
        assert!(cell.check());
        assert!(cell.get().is_some());
    }
}
