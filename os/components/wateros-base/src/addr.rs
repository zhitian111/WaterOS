#[allow(unused)]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BasePhysAddr {
    pub val : usize,
}
#[allow(unused)]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseVirtAddr {
    pub val : usize,
}

#[allow(unused)]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BasePPN {
    pub val : usize,
}
#[allow(unused)]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseVPN {
    pub val : usize,
}

impl<T> Into<*mut T> for BasePhysAddr {
    #[inline]
    #[allow(unused)]
    fn into(self) -> *mut T { self.val as *mut T }
}
impl<T> Into<*mut T> for BaseVirtAddr {
    #[inline]
    #[allow(unused)]
    fn into(self) -> *mut T { self.val as *mut T }
}
