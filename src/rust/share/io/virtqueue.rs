#[derive(Copy, Clone, Debug)]
pub struct Virtqueue {
    pub desc : *mut Descriptor, // 描述符表
    pub avail : *mut AvailRing, // Avail 环
    pub used : *const UsedRing, // Used 环
}

#[derive(Copy, Clone, Debug)]
pub struct Descriptor {
    pub addr : u64,  // 缓冲区地址
    pub len : u32,   // 缓冲区长度
    pub flags : u16, // 标志（如是否有下一个描述符）
    pub next : u16,  // 下一个描述符的索引
}

#[derive(Copy, Clone, Debug)]
pub struct AvailRing {
    pub flags : u16,
    pub idx : u16,         // 当前索引
    pub ring : [u16; 256], // 描述符索引数组
}

#[derive(Copy, Clone, Debug)]
pub struct UsedRing {
    pub flags : u16,
    pub idx : u16,              // 当前索引
    pub ring : [UsedElem; 256], // 已处理元素数组
}

#[derive(Copy, Clone, Debug)]
pub struct UsedElem {
    pub id : u32,  // 描述符链的起始索引
    pub len : u32, // 处理的字节数
}
impl Virtqueue {
    pub fn zeros() -> Self {
        Self { desc : core::ptr::null_mut(),
               avail : core::ptr::null_mut(),
               used : core::ptr::null() }
    }
}
