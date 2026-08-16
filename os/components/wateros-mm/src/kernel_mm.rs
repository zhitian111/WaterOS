//! MM 内核地址空间与 ELF 装载 facade。

/// 用户页错处理结果；trap 层据此选择 Linux 的同步信号语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserPageFaultResult {
    /// 已完成 COW、按需装页或必要的 TLB 修复，可重试原指令。
    Handled,
    /// 地址不属于任何用户映射，对应 `SEGV_MAPERR`。
    MapError,
    /// 地址已映射但本次访问不符合权限，对应 `SEGV_ACCERR`。
    AccessError,
    /// 处理缺页时物理内存耗尽。
    OutOfMemory,
    /// 文件后备页无法装入，对应 `SIGBUS/BUS_ADRERR`。
    BackingError,
    /// 页表或地址空间状态出现其它内部错误。
    Internal(api_v0::error::MmError),
}

pub use api_v0::kernel_bringup::{
        LoadElfError, LoadProgramError, LoadedElf, PrepareUserStackError, RootVolumeReadError, DEFAULT_USER_ELF_PATH,
    };
    pub use api_v0::executable::ExecResolveError;

    pub fn handle_tlb_shootdown_ipi() -> bool {
        #[cfg(feature = "impl-sv39")]
        { return impl_sv39::kernel_mm_impl::handle_tlb_shootdown_ipi(); }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        { return impl_loongarch64::kernel_mm_impl::handle_tlb_shootdown_ipi(); }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        { false }
    }

    /// 在已装载 ELF 的用户栈上写入 argc/argv/envp/auxv，返回初始 `sp`。
    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    pub fn prepare_elf_user_stack(
        elf: &LoadedElf,
        argv: &[&str],
        envp: &[&str],
    ) -> Result<usize, PrepareUserStackError> {
        let ops = crate::ActiveUserMemoryOps::new(elf.user_aspace_ptr);
        api_v0::elf_user_stack::prepare_elf_user_stack(&ops, elf, argv, envp)
    }

    #[cfg(feature = "impl-sv39")]
    pub use impl_sv39::kernel_mm_impl::{
        drop_user_aspace, ensure_user_execute_for_kernel_va, fork_user_aspace, from_elf_bytes,
        from_elf_path, init, kernel_satp, load_program_from_path, map_anon_range_user,
        map_identity_range_user,
    };

    pub fn handle_cow_fault(aspace_ptr: usize,
                            fault_addr: usize)
                            -> api_v0::error::MmResult<bool> {
        #[cfg(feature = "impl-sv39")]
        {
            return impl_sv39::kernel_mm_impl::handle_cow_fault(aspace_ptr, fault_addr);
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            return impl_loongarch64::kernel_mm_impl::handle_cow_fault(aspace_ptr, fault_addr);
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, fault_addr);
            Ok(false)
        }
    }

    pub fn handle_user_page_fault(
        aspace_ptr: usize,
        fault_addr: usize,
        access: api_v0::mmap::PageFaultAccess,
    ) -> UserPageFaultResult {
        #[cfg(feature = "impl-sv39")]
        {
            use api_v0::addr::VirtAddr;
            use api_v0::error::MmError;
            use api_v0::mmap::MmapOps;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            let fault_addr = VirtAddr(fault_addr);
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                let outcome = match MmapOps::handle_page_fault(aspace,
                                                               &mut alloc,
                                                               fault_addr,
                                                               access)
                {
                    Ok(true) => UserPageFaultResult::Handled,
                    Ok(false) if aspace.madvise_range_mapped(fault_addr, 1) => {
                        UserPageFaultResult::AccessError
                    }
                    Ok(false) => UserPageFaultResult::MapError,
                    Err(MmError::OutOfMemory) => UserPageFaultResult::OutOfMemory,
                    Err(MmError::AccessViolation) => UserPageFaultResult::BackingError,
                    Err(error) => UserPageFaultResult::Internal(error),
                };
                Ok(outcome)
            })
            .unwrap_or_else(UserPageFaultResult::Internal);
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            use api_v0::addr::VirtAddr;
            use api_v0::error::MmError;
            use api_v0::mmap::MmapOps;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            let fault_addr = VirtAddr(fault_addr);
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                let outcome = match MmapOps::handle_page_fault(aspace,
                                                               &mut alloc,
                                                               fault_addr,
                                                               access)
                {
                    Ok(true) => UserPageFaultResult::Handled,
                    Ok(false) if aspace.madvise_range_mapped(fault_addr, 1) => {
                        UserPageFaultResult::AccessError
                    }
                    Ok(false) => UserPageFaultResult::MapError,
                    Err(MmError::OutOfMemory) => UserPageFaultResult::OutOfMemory,
                    Err(MmError::AccessViolation) => UserPageFaultResult::BackingError,
                    Err(error) => UserPageFaultResult::Internal(error),
                };
                Ok(outcome)
            })
            .unwrap_or_else(UserPageFaultResult::Internal);
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, fault_addr, access);
            UserPageFaultResult::MapError
        }
    }

    pub fn madvise_discard_pages(aspace_ptr: usize,
                                 addr: usize,
                                 len: usize)
                                 -> api_v0::error::MmResult<()> {
        #[cfg(feature = "impl-sv39")]
        {
            use api_v0::addr::VirtAddr;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            return crate::user_aspace::with_user_aspace_mut_and_flush(aspace_ptr, |aspace| {
                aspace.madvise_discard_mapped_pages(&mut alloc, VirtAddr(addr), len)
            });
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            use api_v0::addr::VirtAddr;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            return crate::user_aspace::with_user_aspace_mut_and_flush(aspace_ptr, |aspace| {
                aspace.madvise_discard_mapped_pages(&mut alloc, VirtAddr(addr), len)
            });
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, addr, len);
            Ok(())
        }
    }

    pub fn madvise_range_mapped(aspace_ptr: usize,
                                addr: usize,
                                len: usize)
                                -> api_v0::error::MmResult<bool> {
        #[cfg(feature = "impl-sv39")]
        {
            use api_v0::addr::VirtAddr;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                Ok(aspace.madvise_range_mapped(VirtAddr(addr), len))
            });
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            use api_v0::addr::VirtAddr;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                Ok(aspace.madvise_range_mapped(VirtAddr(addr), len))
            });
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, addr, len);
            Ok(false)
        }
    }

    /// 验证整个用户区间已被 VMA 覆盖，并把尚未驻留的页按指定访问方式预取。
    ///
    /// WaterOS 当前没有 swap 或匿名页回收，因此 `mlock` 的“禁止换出”天然成立；
    /// 真正还需要完成的是 Linux 要求的当前区间驻留语义。
    pub fn prefault_user_range(aspace_ptr : usize,
                               addr : usize,
                               len : usize,
                               write : bool)
                               -> api_v0::error::MmResult<()> {
        use api_v0::addr::{VirtAddr, VirtPageNum, PAGE_SIZE};
        use api_v0::address_space::AddressSpaceOps;
        use api_v0::error::MmError;
        use api_v0::mmap::{MmapOps, PageFaultAccess};

        let end = addr.checked_add(len).ok_or(MmError::InvalidAddress)?;
        let start = addr & !(PAGE_SIZE - 1);
        let end = end.checked_add(PAGE_SIZE - 1)
                     .ok_or(MmError::InvalidAddress)? &
                  !(PAGE_SIZE - 1);
        let access = if write { PageFaultAccess::Write } else { PageFaultAccess::Read };
        let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;

        #[cfg(feature = "impl-sv39")]
        {
            return crate::user_aspace::with_user_aspace_mut_and_flush(aspace_ptr, |aspace| {
                if !aspace.madvise_range_mapped(VirtAddr(start), end - start) {
                    return Err(MmError::InvalidAddress);
                }
                let mut vpn = VirtAddr(start).floor_page();
                let vpn_end = VirtAddr(end).floor_page();
                while vpn.0 < vpn_end.0 {
                    let page = vpn.start_addr();
                    if aspace.translate_addr(page)?.is_none() &&
                       !MmapOps::handle_page_fault(aspace, &mut alloc, page, access)?
                    {
                        return Err(MmError::InvalidAddress);
                    }
                    vpn = VirtPageNum(vpn.0 + 1);
                }
                Ok(())
            });
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            return crate::user_aspace::with_user_aspace_mut_and_flush(aspace_ptr, |aspace| {
                if !aspace.madvise_range_mapped(VirtAddr(start), end - start) {
                    return Err(MmError::InvalidAddress);
                }
                let mut vpn = VirtAddr(start).floor_page();
                let vpn_end = VirtAddr(end).floor_page();
                while vpn.0 < vpn_end.0 {
                    let page = vpn.start_addr();
                    if aspace.translate_addr(page)?.is_none() &&
                       !MmapOps::handle_page_fault(aspace, &mut alloc, page, access)?
                    {
                        return Err(MmError::InvalidAddress);
                    }
                    vpn = VirtPageNum(vpn.0 + 1);
                }
                Ok(())
            });
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, start, end, write, access, &mut alloc);
            Err(MmError::Unsupported)
        }
    }

    /// 返回区间内各页的驻留位（Linux `mincore` 低位语义）。
    pub fn mincore_user_range(aspace_ptr : usize,
                              addr : usize,
                              len : usize,
                              residency : &mut [u8])
                              -> api_v0::error::MmResult<()> {
        use api_v0::addr::{VirtAddr, VirtPageNum, PAGE_SIZE};
        use api_v0::address_space::AddressSpaceOps;
        use api_v0::error::MmError;

        let end = addr.checked_add(len).ok_or(MmError::InvalidAddress)?;
        let page_count = len.checked_add(PAGE_SIZE - 1)
                            .ok_or(MmError::InvalidAddress)? /
                         PAGE_SIZE;
        if addr & (PAGE_SIZE - 1) != 0 || residency.len() != page_count {
            return Err(MmError::InvalidAddress);
        }
        #[cfg(feature = "impl-sv39")]
        {
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                if !aspace.madvise_range_mapped(VirtAddr(addr), end - addr) {
                    return Err(MmError::InvalidAddress);
                }
                let mut vpn = VirtAddr(addr).floor_page();
                for byte in residency {
                    *byte = u8::from(aspace.translate_addr(vpn.start_addr())?.is_some());
                    vpn = VirtPageNum(vpn.0 + 1);
                }
                Ok(())
            });
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                if !aspace.madvise_range_mapped(VirtAddr(addr), end - addr) {
                    return Err(MmError::InvalidAddress);
                }
                let mut vpn = VirtAddr(addr).floor_page();
                for byte in residency {
                    *byte = u8::from(aspace.translate_addr(vpn.start_addr())?.is_some());
                    vpn = VirtPageNum(vpn.0 + 1);
                }
                Ok(())
            });
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, addr, end, residency);
            Err(MmError::Unsupported)
        }
    }

    /// 预取当前地址空间中的全部 lazy 用户 VMA，供 `mlockall(MCL_CURRENT)`。
    pub fn prefault_all_current_user_ranges(aspace_ptr : usize)
                                             -> api_v0::error::MmResult<()> {
        let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
        #[cfg(feature = "impl-sv39")]
        {
            return crate::user_aspace::with_user_aspace_mut_and_flush(aspace_ptr, |aspace| {
                aspace.prefault_all_current_user_ranges(&mut alloc)
            });
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            return crate::user_aspace::with_user_aspace_mut_and_flush(aspace_ptr, |aspace| {
                aspace.prefault_all_current_user_ranges(&mut alloc)
            });
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, &mut alloc);
            Err(api_v0::error::MmError::Unsupported)
        }
    }

    pub fn madvise_range_shared_or_file(aspace_ptr: usize,
                                        addr: usize,
                                        len: usize)
                                        -> api_v0::error::MmResult<bool> {
        #[cfg(feature = "impl-sv39")]
        {
            use api_v0::addr::VirtAddr;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                Ok(aspace.madvise_range_shared_or_file(VirtAddr(addr), len))
            });
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            use api_v0::addr::VirtAddr;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                Ok(aspace.madvise_range_shared_or_file(VirtAddr(addr), len))
            });
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, addr, len);
            Ok(false)
        }
    }

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::kernel_mm_impl::{
        drop_user_aspace, ensure_user_execute_for_kernel_va, fork_user_aspace, from_elf_bytes,
        from_elf_path, init, kernel_satp, load_program_from_path, map_anon_range_user,
        map_identity_range_user,
    };
