//! RISC-V 用户态初始栈布局（argc / argv / envp / auxv），供 `execve` 与带 argv 的
//! 用户任务 spawn 共用。

extern crate alloc;

use alloc::vec::Vec;

use crate::addr::VirtAddr;
use crate::kernel_bringup::{LoadedElf, PrepareUserStackError};
use crate::user_access::UserMemoryOps;

const AT_NULL: usize = 0;
const AT_PAGESZ: usize = 6;
const AT_PHDR: usize = 3;
const AT_PHENT: usize = 4;
const AT_PHNUM: usize = 5;
const AT_ENTRY: usize = 9;
const AT_RANDOM: usize = 25;

const PAGE_SIZE: usize = 4096;

fn push_to_user_stack<Ops: UserMemoryOps>(
    ops: &Ops,
    sp: &mut usize,
    data: &[u8],
) -> Result<(), PrepareUserStackError> {
    let aligned_len = (data.len() + 15) & !15;
    *sp = sp
        .checked_sub(aligned_len)
        .ok_or(PrepareUserStackError::StackOverflow)?;
    let mut buf = Vec::with_capacity(aligned_len);
    buf.resize(aligned_len, 0u8);
    buf[..data.len()].copy_from_slice(data);
    ops.copy_to_user(VirtAddr(*sp), &buf)
        .map_err(|_| PrepareUserStackError::AccessViolation)?;
    Ok(())
}

fn build_auxv(elf: &LoadedElf) -> Vec<usize> {
    alloc::vec![
        AT_PAGESZ,
        PAGE_SIZE,
        AT_PHDR,
        elf.image_base + 0x40,
        AT_PHENT,
        56,
        AT_PHNUM,
        7,
        AT_ENTRY,
        elf.entry_pc,
        AT_RANDOM,
        0,
        AT_NULL,
        0,
    ]
}

fn usize_pair_to_bytes(pair: [usize; 2]) -> [u8; 16] {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&pair[0].to_le_bytes());
    buf[8..].copy_from_slice(&pair[1].to_le_bytes());
    buf
}

/// 在 `elf` 的用户栈上构造 argc/argv/envp/auxv，返回首次进入用户态时的 `sp`。
pub fn prepare_elf_user_stack<Ops: UserMemoryOps>(
    ops: &Ops,
    elf: &LoadedElf,
    argv: &[&str],
    envp: &[&str],
) -> Result<usize, PrepareUserStackError> {
    if elf.user_aspace_ptr == 0 {
        return Err(PrepareUserStackError::NoUserAspace);
    }
    let mut sp = elf.stack_top;

    let mut argv_addrs: Vec<usize> = Vec::new();
    for s in argv {
        let bytes = s.as_bytes();
        let mut blob = Vec::with_capacity(bytes.len() + 1);
        blob.extend_from_slice(bytes);
        blob.push(0);
        push_to_user_stack(ops, &mut sp, &blob)?;
        argv_addrs.push(sp);
    }

    let mut envp_addrs: Vec<usize> = Vec::new();
    for s in envp {
        let bytes = s.as_bytes();
        let mut blob = Vec::with_capacity(bytes.len() + 1);
        blob.extend_from_slice(bytes);
        blob.push(0);
        push_to_user_stack(ops, &mut sp, &blob)?;
        envp_addrs.push(sp);
    }

    let auxv = build_auxv(elf);
    sp &= !15;

    for chunk in auxv.chunks(2).rev() {
        let pair = [
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(0).copied().unwrap_or(0),
        ];
        push_to_user_stack(ops, &mut sp, &usize_pair_to_bytes(pair))?;
    }

    push_to_user_stack(ops, &mut sp, &0usize.to_le_bytes())?;
    for &addr in envp_addrs.iter().rev() {
        push_to_user_stack(ops, &mut sp, &addr.to_le_bytes())?;
    }

    push_to_user_stack(ops, &mut sp, &0usize.to_le_bytes())?;
    for &addr in argv_addrs.iter().rev() {
        push_to_user_stack(ops, &mut sp, &addr.to_le_bytes())?;
    }

    push_to_user_stack(ops, &mut sp, &argv.len().to_le_bytes())?;

    Ok(sp)
}
