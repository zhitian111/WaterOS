//! NUMA 内存策略语义（bring-up：单节点 stub，无 per-vma 状态）。

/// 默认内存策略：由内核选择节点；单节点实现中唯一有效的 mode。
pub const MPOL_DEFAULT: i32 = 0;

/// 请求返回首选节点编号；单节点时返回节点 0。
pub const MPOL_F_NODE: usize = 1;
/// `MPOL_F_ADDR`
pub const MPOL_F_ADDR: usize = 2;
/// `MPOL_F_MEMS_ALLOWED`
pub const MPOL_F_MEMS_ALLOWED: usize = 4;

/// bring-up 支持的 `get_mempolicy` flags 位掩码。
pub const MPOL_VALID_FLAGS: usize = MPOL_F_NODE | MPOL_F_ADDR | MPOL_F_MEMS_ALLOWED;

/// 内存策略操作错误（聚合层映射 errno）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MempolicyError {
    /// flags、`maxnode` 或调用组合不符合当前支持的单节点子集。
    InvalidArg,
}

/// 单节点、无 NUMA 拓扑时的策略实现（纯逻辑，不涉及页表 walk）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SingleNodeMempolicy;

impl SingleNodeMempolicy {
    /// 校验 `get_mempolicy(2)` flags。
    pub fn validate_get_flags(flags: usize) -> Result<(), MempolicyError> {
        if flags & !MPOL_VALID_FLAGS != 0 {
            Err(MempolicyError::InvalidArg)
        } else {
            Ok(())
        }
    }

    /// 默认线程/地址策略 mode。
    #[must_use]
    pub const fn default_mode() -> i32 {
        MPOL_DEFAULT
    }

    /// 计算 nodemask 写入字节数；`maxnode == 0` 且需要写 mask 时非法。
    pub fn nodemask_byte_len(maxnode: usize) -> Result<usize, MempolicyError> {
        if maxnode == 0 {
            return Err(MempolicyError::InvalidArg);
        }
        Ok((maxnode + 7) / 8)
    }

    /// 将 node 0 写入 `buf`（调用方应已清零）。
    /// 空缓冲保持不变；调用方应先按 `nodemask_byte_len` 校验容量，避免把短缓冲误报为完整掩码。
    pub fn fill_nodemask_node0(buf: &mut [u8]) {
        if !buf.is_empty() {
            buf[0] |= 1;
        }
    }
}
