//! RISC-V 用户态初始栈布局（argc / argv / envp / auxv），供 `execve` 与带 argv 的
//! 用户任务 spawn 共用。

extern crate alloc;

use alloc::vec::Vec;

use crate::addr::VirtAddr;
use crate::kernel_bringup::{LoadedElf, PrepareUserStackError};
use crate::user_access::UserMemoryOps;

const AT_NULL : usize = 0;
const AT_PHDR : usize = 3;
const AT_PHENT : usize = 4;
const AT_PHNUM : usize = 5;
const AT_PAGESZ : usize = 6;
const AT_BASE : usize = 7;
const AT_ENTRY : usize = 9;
const AT_UID : usize = 11;
const AT_EUID : usize = 12;
const AT_GID : usize = 13;
const AT_EGID : usize = 14;
const AT_HWCAP : usize = 16;
const AT_SECURE : usize = 23;
const AT_RANDOM : usize = 25;

const PAGE_SIZE : usize = 4096;

fn push_to_user_stack<Ops : UserMemoryOps>(ops : &Ops,
                                           sp : &mut usize,
                                           data : &[u8])
                                           -> Result<(), PrepareUserStackError> {
    let aligned_len = (data.len() + 15) & !15;
    *sp = sp.checked_sub(aligned_len)
            .ok_or(PrepareUserStackError::StackOverflow)?;
    let mut buf = Vec::with_capacity(aligned_len);
    buf.resize(aligned_len, 0u8);
    buf[..data.len()].copy_from_slice(data);
    ops.copy_to_user(VirtAddr(*sp), &buf)
       .map_err(|_| PrepareUserStackError::AccessViolation)?;
    Ok(())
}

fn push_user_word<Ops : UserMemoryOps>(ops : &Ops,
                                       sp : &mut usize,
                                       word : usize)
                                       -> Result<(), PrepareUserStackError> {
    *sp = sp.checked_sub(core::mem::size_of::<usize>())
            .ok_or(PrepareUserStackError::StackOverflow)?;
    ops.copy_to_user(VirtAddr(*sp), &word.to_le_bytes())
       .map_err(|_| PrepareUserStackError::AccessViolation)?;
    Ok(())
}

fn build_auxv(elf : &LoadedElf, random_addr : usize) -> Vec<usize> {
    alloc::vec![AT_PAGESZ,
                PAGE_SIZE,
                AT_PHDR,
                elf.phdr_va,
                AT_PHENT,
                elf.phentsize,
                AT_PHNUM,
                elf.phnum,
                AT_BASE,
                elf.interp_base,
                AT_ENTRY,
                elf.program_entry,
                AT_UID,
                0,
                AT_EUID,
                0,
                AT_GID,
                0,
                AT_EGID,
                0,
                AT_HWCAP,
                elf_hwcap(),
                AT_SECURE,
                0,
                AT_RANDOM,
                random_addr,
                AT_NULL,
                0,]
}

fn elf_hwcap() -> usize {
    #[cfg(target_arch = "riscv64")]
    {
        // RISC-V HWCAP: bit 0='a', bit 1='c', bit 2='d', bit 3='f',
        // bit 4='i', bit 5='m'. Do not advertise vector support here.
        0b0011_1111
    }
    #[cfg(target_arch = "loongarch64")]
    {
        // LoongArch HWCAP uses different bit assignments from RISC-V. Keep the
        // advertised set conservative so glibc does not select LSX/LASX paths
        // before the kernel saves/restores those extension registers.
        const HWCAP_LOONGARCH_FPU : usize = 1 << 3;
        HWCAP_LOONGARCH_FPU
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
    {
        0
    }
}

fn usize_pair_to_bytes(pair : [usize; 2]) -> [u8; 16] {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&pair[0].to_le_bytes());
    buf[8..].copy_from_slice(&pair[1].to_le_bytes());
    buf
}

/// 在 `elf` 的用户栈上构造 argc/argv/envp/auxv，返回首次进入用户态时的 `sp`。
pub fn prepare_elf_user_stack<Ops : UserMemoryOps>(ops : &Ops,
                                                   elf : &LoadedElf,
                                                   argv : &[&str],
                                                   envp : &[&str])
                                                   -> Result<usize, PrepareUserStackError> {
    if elf.user_aspace_ptr == 0 {
        return Err(PrepareUserStackError::NoUserAspace);
    }
    let mut sp = elf.stack_top;

    let mut argv_addrs : Vec<usize> = Vec::new();
    for s in argv {
        let bytes = s.as_bytes();
        let mut blob = Vec::with_capacity(bytes.len() + 1);
        blob.extend_from_slice(bytes);
        blob.push(0);
        push_to_user_stack(ops, &mut sp, &blob)?;
        argv_addrs.push(sp);
    }

    let mut envp_addrs : Vec<usize> = Vec::new();
    for s in envp {
        let bytes = s.as_bytes();
        let mut blob = Vec::with_capacity(bytes.len() + 1);
        blob.extend_from_slice(bytes);
        blob.push(0);
        push_to_user_stack(ops, &mut sp, &blob)?;
        envp_addrs.push(sp);
    }

    let random = [0x42u8; 16];
    push_to_user_stack(ops, &mut sp, &random)?;
    let random_addr = sp;
    let auxv = build_auxv(elf, random_addr);
    sp &= !15;

    let word_count = 1 + argv_addrs.len() + 1 + envp_addrs.len() + 1 + auxv.len();
    if word_count % 2 != 0 {
        push_user_word(ops, &mut sp, 0)?;
    }

    for chunk in auxv.chunks(2).rev() {
        let pair = [chunk.get(0)
                         .copied()
                         .unwrap_or(0),
                    chunk.get(1)
                         .copied()
                         .unwrap_or(0)];
        push_to_user_stack(ops, &mut sp, &usize_pair_to_bytes(pair))?;
    }

    push_user_word(ops, &mut sp, 0)?;
    for &addr in envp_addrs.iter()
                           .rev()
    {
        push_user_word(ops, &mut sp, addr)?;
    }

    push_user_word(ops, &mut sp, 0)?;
    for &addr in argv_addrs.iter()
                           .rev()
    {
        push_user_word(ops, &mut sp, addr)?;
    }

    push_user_word(ops, &mut sp, argv.len())?;

    Ok(sp)
}
