//! MM 内核地址空间与 ELF 装载 facade。

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

    pub fn handle_cow_fault(aspace_ptr: usize, fault_addr: usize) -> bool {
        #[cfg(feature = "impl-sv39")]
        {
            return impl_sv39::kernel_mm_impl::handle_cow_fault(aspace_ptr, fault_addr)
                .unwrap_or(false);
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            return impl_loongarch64::kernel_mm_impl::handle_cow_fault(aspace_ptr, fault_addr)
                .unwrap_or(false);
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, fault_addr);
            false
        }
    }

    pub fn handle_user_page_fault(
        aspace_ptr: usize,
        fault_addr: usize,
        access: api_v0::mmap::PageFaultAccess,
    ) -> bool {
        #[cfg(feature = "impl-sv39")]
        {
            use api_v0::addr::VirtAddr;
            use api_v0::mmap::MmapOps;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                MmapOps::handle_page_fault(aspace, &mut alloc, VirtAddr(fault_addr), access)
            })
            .unwrap_or(false);
        }
        #[cfg(all(not(feature = "impl-sv39"), feature = "impl-loongarch64"))]
        {
            use api_v0::addr::VirtAddr;
            use api_v0::mmap::MmapOps;
            let mut alloc = crate::frame_alloctor::GlobalPhysFrameAllocator;
            return crate::user_aspace::with_user_aspace_mut(aspace_ptr, |aspace| {
                MmapOps::handle_page_fault(aspace, &mut alloc, VirtAddr(fault_addr), access)
            })
            .unwrap_or(false);
        }
        #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
        {
            let _ = (aspace_ptr, fault_addr, access);
            false
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
