//! NUMA 内存策略原语（单节点 bring-up）；地址校验经活动地址空间 impl。

use crate::api::addr::VirtAddr;
use crate::api::address_space::AddressSpaceOps;
use crate::api::error::MmError;
use crate::api::mempolicy::{MempolicyError, SingleNodeMempolicy};

use crate::user_aspace;

/// 校验用户虚拟地址在当前地址空间内已映射（`MPOL_F_ADDR` 路径）。
pub fn is_user_addr_mapped(user_aspace_handle: usize, addr: usize) -> Result<bool, MmError> {
    if addr == 0 {
        return Ok(false);
    }
    if user_aspace_handle == 0 {
        return Ok(false);
    }
    user_aspace::with_user_aspace_mut(user_aspace_handle, |aspace| {
        aspace.translate_addr(VirtAddr(addr))
    })
    .map(|opt| opt.is_some())
}

/// 单节点 `get_mempolicy` 逻辑结果（不含用户指针拷贝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetMempolicyResult {
    /// 写入 `*mode` 的值（`MPOL_F_NODE` 时不使用）。
    pub mode: i32,
    /// nodemask 字节数；0 表示不写 nodemask。
    /// 按 `maxnode` 计算的掩码字节数；调用方必须据此检查用户缓冲区容量。
    pub nodemask_len: usize,
}

/// 计算 `get_mempolicy` 内核侧结果；`write_nodemask` 为 true 时校验并计算 mask 长度。
pub fn get_mempolicy_single_node(
    flags: usize,
    maxnode: usize,
    write_nodemask: bool,
) -> Result<GetMempolicyResult, MempolicyError> {
    SingleNodeMempolicy::validate_get_flags(flags)?;
    let nodemask_len = if write_nodemask {
        SingleNodeMempolicy::nodemask_byte_len(maxnode)?
    } else {
        0
    };
    Ok(GetMempolicyResult {
        mode: SingleNodeMempolicy::default_mode(),
        nodemask_len,
    })
}

/// 填充 nodemask 缓冲区（长度须为 `nodemask_len`）。
pub fn fill_get_mempolicy_nodemask(buf: &mut [u8]) {
    for byte in buf.iter_mut() {
        *byte = 0;
    }
    SingleNodeMempolicy::fill_nodemask_node0(buf);
}
