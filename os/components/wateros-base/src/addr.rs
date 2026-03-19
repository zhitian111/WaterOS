#[allow(unused)]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BasePhysAddr {
    pub val: usize,
}

#[allow(unused)]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseVirtAddr {
    pub val: usize,
}
