use core::{fmt::Debug, option::Option};
pub trait PlatformBootContext<BootArgs : PlatformBootArgs>: From<BootArgs> {}
pub trait PlatformBootArgs: Debug + Clone + Copy {
    #[inline]
    fn arg0(&self) -> Option<usize> { None }
    #[inline]
    fn arg1(&self) -> Option<usize> { None }
    #[inline]
    fn arg2(&self) -> Option<usize> { None }
    // etc.
}
// Also you need to implement a _start function.
