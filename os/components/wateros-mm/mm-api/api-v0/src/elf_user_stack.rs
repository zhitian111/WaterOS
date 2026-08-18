//! 用户态初始栈布局（argc / argv / envp / auxv），供 `execve` 与带 argv 的
//! 用户任务 spawn 共用。

extern crate alloc;

use alloc::vec::Vec;

use crate::addr::VirtAddr;
use crate::kernel_bringup::{LoadedElf, PrepareUserStackError};
use crate::user_access::UserMemoryOps;

/// auxv 终止键；其值也必须为 0。
const AT_NULL : usize = 0;
/// ELF 程序头表在用户地址空间中的虚拟地址。
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

/// 向用户 ABI 报告的页面大小；必须与 MM API 的 4 KiB 页粒度保持一致。
const PAGE_SIZE : usize = 4096;

#[cfg(any(target_arch = "loongarch64", test))]
const HWCAP_LOONGARCH_UAL : usize = 1 << 2;
#[cfg(any(target_arch = "loongarch64", test))]
const HWCAP_LOONGARCH_FPU : usize = 1 << 3;

#[cfg(any(target_arch = "loongarch64", test))]
const fn loongarch_elf_hwcap() -> usize { HWCAP_LOONGARCH_UAL | HWCAP_LOONGARCH_FPU }

fn push_to_user_stack<Ops : UserMemoryOps>(ops : &Ops,
                                           sp : &mut usize,
                                           data : &[u8])
                                           -> Result<(), PrepareUserStackError> {
    // 将一段字节向下压入用户栈并保持 16 字节对齐；分配临时零填充确保对齐尾部不会泄露内核数据。
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
    // 压入一个目标 ABI 宽度的机器字；WaterOS 当前只支持 64 位目标，序列化为小端字节。
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
        // RISC-V HWCAP 的低六位分别表示 a/c/d/f/i/m 扩展；未保存向量寄存器，故绝不能宣称支持向量扩展。
        0b0011_1111
    }
    #[cfg(target_arch = "loongarch64")]
    {
        // LoongArch 的位分配与 RISC-V 不同。仅报告已保存/恢复的基础能力，避免 glibc 在内核尚未
        // 保存 LSX/LASX 寄存器时选择对应路径；QEMU la464 公开 UAL，且其 TCG 后端需要 auxv 报告它。
        loongarch_elf_hwcap()
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

/// 初始用户栈及内核保留的 auxv 快照。
///
/// `/proc/<pid>/auxv` 必须返回 exec 时的真实向量，而不能在读取时根据已经
/// 变化的进程状态重新拼装，因此构造用户栈时同步保留一份原始字节。
pub struct PreparedUserStack {
    /// 首次返回用户态时写入 trap frame 的栈顶地址，满足目标 ABI 的 16 字节对齐要求。
    pub sp : usize,
    /// exec 时生成的原始 auxv 字节序列，供 `/proc/<pid>/auxv` 读取；按目标机器字小端编码。
    pub auxv : Vec<u8>,
}

/// 在 `elf` 的用户栈上构造 argc/argv/envp/auxv，返回首次进入用户态时的 `sp`。
pub fn prepare_elf_user_stack<Ops : UserMemoryOps>(ops : &Ops,
                                                   elf : &LoadedElf,
                                                   argv : &[&str],
                                                   envp : &[&str])
                                                   -> Result<PreparedUserStack, PrepareUserStackError> {
    // 先检查地址空间上下文，避免对零地址或尚未安装的用户页表执行任何用户拷贝。
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

    // 当前启动链尚无可靠熵源，先提供固定占位；安全执行环境接入熵源后必须替换，不能把它当作 ASLR 秘密。
    let random = [0x42u8; 16];
    push_to_user_stack(ops, &mut sp, &random)?;
    let random_addr = sp;
    let auxv = build_auxv(elf, random_addr);
    let mut auxv_bytes = Vec::with_capacity(auxv.len() * core::mem::size_of::<usize>());
    for word in &auxv {
        auxv_bytes.extend_from_slice(&word.to_le_bytes());
    }
    // System V 64 位 ABI 要求进入程序时栈按 16 字节对齐。
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

    Ok(PreparedUserStack { sp, auxv : auxv_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loongarch_hwcap_advertises_tcg_host_requirements() {
        let hwcap = loongarch_elf_hwcap();
        assert_ne!(hwcap & HWCAP_LOONGARCH_UAL, 0);
        assert_ne!(hwcap & HWCAP_LOONGARCH_FPU, 0);
        assert_eq!(hwcap & !((1 << 2) | (1 << 3)), 0,
                   "do not advertise unsaved LSX/LASX or unrelated extensions");
    }
}
