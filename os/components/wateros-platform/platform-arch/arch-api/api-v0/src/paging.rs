//! Architecture-neutral shape of a local TLB invalidation request.
//!
//! PLATFORM_BOUNDARY: 本枚举只描述**本 CPU**要刷新的翻译范围；向其他 active CPU
//! 发 IPI、等待 ack、页表锁和物理页回收顺序属于 MM/SMP 上层协议。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlbFlushRange {
    /// 刷新当前 CPU 的全部地址翻译缓存。
    All,
    /// 刷新一个地址空间；`token` 的 ASID/根页表编码由 arch impl 解释。
    AddressSpace { token: usize },
    /// 刷新当前地址空间内的单个虚拟页。
    Page { addr: usize },
    /// 请求刷新一段地址；不支持范围刷新的 arch 可保守退化为 `All`。
    Range { start: usize, end: usize },
}
