//! `execve(2)` — 替换当前进程映像。
//!
//! 加载新 ELF、销毁旧地址空间、构造用户栈（argv/envp/auxv）、关闭 CLOEXEC fd，
//! 最后更新 TCB 使当前任务跳转到新程序入口。

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::copy_nonoverlapping;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use mm::api::kernel_bringup::LoadedElf;
use mm::api::user_access::UserMemoryOps;
use mm::ActiveUserMemoryOps;

use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

// ── auxv 常量 ─────────────────────────────────────────────────────

const AT_NULL : usize = 0;
const AT_PAGESZ : usize = 6;
const AT_PHDR : usize = 3;
const AT_PHENT : usize = 4;
const AT_PHNUM : usize = 5;
const AT_ENTRY : usize = 9;
const AT_RANDOM : usize = 25;
const CLOCK_REALTIME : usize = 8;

const PAGE_SIZE : usize = 4096;

// ── stack 辅助 ────────────────────────────────────────────────────

/// 将 `data` 写入 `sp` 下方的用户栈，返回新的 sp（向下增长）。
fn push_to_user_stack(ops : &ActiveUserMemoryOps,
                      sp : &mut usize,
                      data : &[u8])
                      -> Result<(), ErrNo> {
    let aligned_len = (data.len() + 15) & !15; // 16 字节对齐
    *sp = sp.checked_sub(aligned_len)
            .ok_or(ErrNo::EFAULT)?;
    let mut buf = Vec::with_capacity(aligned_len);
    buf.resize(aligned_len, 0u8);
    buf[..data.len()].copy_from_slice(data);
    ops.copy_to_user(mm::api::addr::VirtAddr(*sp), &buf)
       .map_err(|_| ErrNo::EFAULT)?;
    Ok(())
}

// ── auxv 构造 ────────────────────────────────────────────────────

fn build_auxv(elf : &LoadedElf) -> Vec<usize> {
    alloc::vec![AT_PAGESZ,
                PAGE_SIZE,
                AT_PHDR,
                elf.image_base + 0x40, // ELF program headers offset
                AT_PHENT,
                56, // 64-bit ELF phdr size
                AT_PHNUM,
                7, // typical phnum
                AT_ENTRY,
                elf.entry_pc,
                AT_RANDOM,
                0, // no AT_RANDOM for now
                AT_NULL,
                0,]
}

// ── 公开入口 ─────────────────────────────────────────────────────

pub(crate) fn sys_execve(args : SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let argv_ptr = args.arg(1);
    let envp_ptr = args.arg(2);

    match do_execve(path_ptr, argv_ptr, envp_ptr) {
        Ok(()) => {
            // execve 成功后 trap_handler 会将 a0 设为此返回值并 sret 到新程序入口。
            // sepc 已在 TCB 中预减 4 以抵消 add_user_pc(4)。
            UserRet::from_success(0)
        }
        Err(e) => UserRet::from_error(e),
    }
}

fn do_execve(path_ptr : usize, argv_ptr : usize, envp_ptr : usize) -> Result<(), ErrNo> {
    // 1. 读取路径
    let path = copy_user_path_cstr(path_ptr, 256)?;

    // 2. 解析相对路径为绝对路径（通过当前任务 cwd）
    let abs_path = vfs::cwd::resolve_for_current_task(&path).unwrap_or(path);

    // 3. 从用户内存收集 argv/envp 字符串（旧地址空间）
    let argv = read_string_array(argv_ptr)?;
    let envp = read_string_array(envp_ptr)?;

    // 4. 加载新 ELF → 新地址空间
    let new_elf = mm::kernel_mm::from_elf_path(&abs_path).map_err(|_| ErrNo::ENOENT)?;

    // 5. 在新地址空间构造用户栈
    let new_sp = build_user_stack(&new_elf, &argv, &envp)?;

    // 6. 销毁旧地址空间
    let old_aspace = task::current_task_user_aspace_ptr();
    mm::kernel_mm::drop_user_aspace(old_aspace);

    // 7. 关闭带 FD_CLOEXEC 的 fd
    vfs::fd::close_cloexec_fds_for_current_task()
        .map_err(vfs_error_to_errno)?;

    // 8. 更新 TCB：替换地址空间、入口、栈
    let image_info = task::UserImageInfo::new(new_elf.image_base, new_elf.image_size);
    let stack_info = task::UserStack::from_range(new_elf.stack_bottom, new_elf.stack_top);
    task::execve_current(new_elf.entry_pc,
                         new_sp,
                         new_elf.satp,
                         new_elf.user_aspace_ptr,
                         image_info,
                         stack_info);

    // 控制流应在此之后直接 sret 到新程序，但 syscall 返回路径会继续执行。
    // trap_handler 会用更新后的 trap_frame 做 sret。
    Ok(())
}

// ── argv/envp 读取 ────────────────────────────────────────────────

/// 从用户态 `char **` 数组读取所有字符串。
fn read_string_array(array_ptr : usize) -> Result<Vec<String>, ErrNo> {
    let mut result = Vec::new();
    if array_ptr == 0 {
        return Ok(result);
    }
    let ops = ActiveUserMemoryOps::new(task::current_task_user_aspace_ptr());
    let mut ptr_size = [0u8; 8];
    loop {
        if ops.copy_from_user(&mut ptr_size,
                              mm::api::addr::VirtAddr(array_ptr + result.len() * 8))
              .is_err()
        {
            return Ok(result);
        }
        let ptr = usize::from_le_bytes(ptr_size);
        if ptr == 0 {
            break;
        }
        match copy_user_path_cstr(ptr, 256) {
            Ok(s) => result.push(s),
            Err(_) => break,
        }
    }
    Ok(result)
}

// ── 栈布局 ────────────────────────────────────────────────────────

/// 在新地址空间中构造 RISC-V execve 用户栈。
///
/// 栈布局（高地址 → 低地址）：
/// ```text
///   [auxv][0]    ← AT_NULL
///   auxv[n-1]
///   ...
///   auxv[0]
///   [envp][0]    ← NULL
///   envp[n-1]
///   ...
///   envp[0]
///   [argv][0]    ← NULL
///   argv[argc-1]
///   ...
///   argv[0]
///   argc
///   [16-byte align + strings] ...
/// sp →
/// ```
fn build_user_stack(elf : &LoadedElf, argv : &[String], envp : &[String]) -> Result<usize, ErrNo> {
    let ops = ActiveUserMemoryOps::new(elf.user_aspace_ptr);
    let mut sp = elf.stack_top;

    // 把字符串数据压入栈
    let mut argv_addrs : Vec<usize> = Vec::new();
    for s in argv.iter() {
        let bytes = s.as_bytes();
        push_to_user_stack(&ops, &mut sp, &[bytes, &[0u8]].concat())?;
        argv_addrs.push(sp);
    }

    let mut envp_addrs : Vec<usize> = Vec::new();
    for s in envp.iter() {
        let bytes = s.as_bytes();
        push_to_user_stack(&ops, &mut sp, &[bytes, &[0u8]].concat())?;
        envp_addrs.push(sp);
    }

    let auxv = build_auxv(elf);

    // 16 字节对齐
    sp = sp & !15;

    // auxv（从后往前：AT_NULL 在最靠近高地址处）
    for chunk in auxv.chunks(2).rev() {
        let pair = [chunk.get(1)
                         .copied()
                         .unwrap_or(0),
                    chunk.get(0)
                         .copied()
                         .unwrap_or(0)];
        push_to_user_stack(&ops,
                           &mut sp,
                           &usize_pair_to_bytes(pair))?;
    }

    // envp 指针数组（NULL 结尾）
    push_to_user_stack(&ops, &mut sp, &0usize.to_le_bytes())?;
    for &addr in envp_addrs.iter()
                           .rev()
    {
        push_to_user_stack(&ops, &mut sp, &addr.to_le_bytes())?;
    }

    // argv 指针数组（NULL 结尾）
    push_to_user_stack(&ops, &mut sp, &0usize.to_le_bytes())?;
    for &addr in argv_addrs.iter()
                           .rev()
    {
        push_to_user_stack(&ops, &mut sp, &addr.to_le_bytes())?;
    }

    // argc
    push_to_user_stack(&ops,
                       &mut sp,
                       &argv.len()
                            .to_le_bytes())?;

    Ok(sp)
}

fn usize_pair_to_bytes(pair : [usize; 2]) -> [u8; 16] {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&pair[0].to_le_bytes());
    buf[8..].copy_from_slice(&pair[1].to_le_bytes());
    buf
}
