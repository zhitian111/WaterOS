//! 与架构无关的本地 TLB 失效请求描述。
//!
//! 平台边界：本枚举只描述**本 CPU**要刷新的翻译范围；向其他 active CPU 发 IPI、
//! 等待 ack、页表锁和物理页回收顺序属于 MM/SMP 上层协议。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlbFlushRange {
    /// 刷新当前 CPU 的全部地址翻译缓存。
    All,
    /// 刷新一个地址空间；`token` 的 ASID/根页表编码由 arch impl 解释。
    AddressSpace {
        /// 地址空间 token；其 ASID/根页表编码由架构实现解释。
        token: usize,
    },
    /// 刷新当前地址空间内的单个虚拟页。
    Page {
        /// 要刷新的页内虚拟地址，通常要求按页对齐。
        addr: usize,
    },
    /// 请求刷新一段地址；不支持范围刷新的 arch 可保守退化为 `All`。
    Range {
        /// 起始虚拟地址（含）。
        start: usize,
        /// 结束虚拟地址（不含）；实现需处理反向或溢出区间。
        end: usize,
    },
}
