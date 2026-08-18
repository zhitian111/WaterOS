//! 用户缓冲区访问：通常在 **trap 处理 / syscall 内核路径** 中调用，此时应已持有正确的地址空间上下文（或能推导当前任务页表）。

use crate::addr::VirtAddr;
use crate::error::{MmError, MmResult};

/// 非 private futex 地址在当前映射中的稳定身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutexMappingIdentity {
    /// 映射不跨地址空间共享，futex 应按当前地址空间和用户 VA 建键。
    Private,
    /// 映射跨地址空间共享，值为共享字的稳定物理身份。
    Shared(usize),
}

/// 内核向用户空间写入时已经完成的前缀及随后发生的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserCopyProgress {
    /// 在出错前已经成功写入的连续字节数；不会包含失败页中的任何未确认字节。
    pub copied : usize,
    /// `None` 表示全部成功；`Some` 表示后续字节未写入，调用方可据此实现部分成功 ABI。
    pub error : Option<MmError>,
}

impl UserCopyProgress {
    #[inline]
    pub const fn complete(copied : usize) -> Self {
        Self { copied,
               error : None }
    }

    #[inline]
    pub const fn failed(copied : usize, error : MmError) -> Self {
        Self { copied,
               error : Some(error) }
    }
}

/// 用户态内存访问（用于 syscall、glibc、文件系统等）。
///
/// 实现应当：
/// - 将 `VirtAddr` 翻译到 **当前** 用户地址空间的物理映射（页大小与 [`crate::addr::PAGE_SIZE`] 一致）；
/// - 处理跨页拷贝与权限校验（未映射页不得静默成功）；
/// - 对未映射/越权返回合适的 [`crate::error::MmError`]。
pub trait UserMemoryOps {
    /// 从用户虚拟地址 `src` 起读取，写入 `dst`；可跨页，遇未映射或权限不足返回错误。
    ///
    /// 成功返回实际拷贝的字节数（当前 API 与全量成功语义一致时可等于 `dst.len()`，以实现为准）。
    fn copy_from_user(&self, dst : &mut [u8], src : VirtAddr) -> MmResult<usize>;

    /// 将 `src` 写入用户缓冲区 `dst` 起始处，保留错误前已复制的精确前缀。
    ///
    /// 空输入必须返回 `{ copied: 0, error: None }` 且不访问 `dst`。每一页都必须在
    /// 写入前完成缺页/COW 处理、权限检查和物理地址翻译。
    fn copy_to_user_progress(&self, dst : VirtAddr, src : &[u8]) -> UserCopyProgress;

    /// 将 `src` 完整写入用户缓冲区 `dst`；可跨页。
    ///
    /// 此兼容接口保持原有全量成功语义。需要处理中途进度的调用方应使用
    /// [`Self::copy_to_user_progress`]。
    fn copy_to_user(&self, dst : VirtAddr, src : &[u8]) -> MmResult<usize> {
        let progress = self.copy_to_user_progress(dst, src);
        match progress.error {
            Some(error) => Err(error),
            None => Ok(progress.copied),
        }
    }

    /// 原子读取一个四字节对齐的用户 `u32`。
    /// 地址未对齐、跨页不可原子访问或无读权限时必须返回错误，不能退化成普通非原子拷贝。
    fn atomic_load_u32(&self, src : VirtAddr) -> MmResult<u32>;

    /// 当用户 `u32` 等于 `expected` 时原子写入 `desired`，无论交换是否
    /// 成功都返回操作时观察到的旧值。实现需提供与 futex 等待/唤醒协议相容的原子性，而不仅是关中断。
    fn atomic_compare_exchange_u32(&self,
                                   dst : VirtAddr,
                                   expected : u32,
                                   desired : u32)
                                   -> MmResult<u32>;

    /// 返回非 private futex 使用的映射身份。
    ///
    /// 私有/COW 映射必须返回 [`FutexMappingIdentity::Private`]，避免首次写入
    /// 改变物理页后丢失已有等待队列；真正共享映射返回稳定共享字身份。
    fn futex_mapping_identity_u32(&self, src : VirtAddr) -> MmResult<FutexMappingIdentity>;
}

pub fn test() {
    struct PartialWrite;

    impl UserMemoryOps for PartialWrite {
        fn copy_from_user(&self, _dst : &mut [u8], _src : VirtAddr) -> MmResult<usize> {
            Err(MmError::Unsupported)
        }

        fn copy_to_user_progress(&self, _dst : VirtAddr, _src : &[u8]) -> UserCopyProgress {
            UserCopyProgress::failed(3, MmError::AccessViolation)
        }

        fn atomic_load_u32(&self, _src : VirtAddr) -> MmResult<u32> { Err(MmError::Unsupported) }

        fn atomic_compare_exchange_u32(&self,
                                       _dst : VirtAddr,
                                       _expected : u32,
                                       _desired : u32)
                                       -> MmResult<u32> {
            Err(MmError::Unsupported)
        }

        fn futex_mapping_identity_u32(&self,
                                      _src : VirtAddr)
                                      -> MmResult<FutexMappingIdentity> {
            Err(MmError::Unsupported)
        }
    }

    let progress = PartialWrite.copy_to_user_progress(VirtAddr(0), &[0; 8]);
    assert_eq!(progress,
               UserCopyProgress::failed(3, MmError::AccessViolation));
    assert_eq!(PartialWrite.copy_to_user(VirtAddr(0), &[0; 8]),
               Err(MmError::AccessViolation));
}
