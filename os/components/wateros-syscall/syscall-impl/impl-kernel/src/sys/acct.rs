//! `acct(2)`：进程 accounting 兼容入口。

extern crate alloc;

use alloc::string::String;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use spin::Mutex;
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsNodeType, VfsOpenFlags, VfsOpenOps};

use crate::sys::path_at::{resolve_final_symlink, resolve_path_at, AT_FDCWD};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const ACCT_COMM: usize = 16;
const ACCT_VERSION: u8 = 2;
const ACCT_AHZ: u16 = 100;

static ACCOUNTING_PATH: Mutex<Option<String>> = Mutex::new(None);

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxAcct {
    ac_flag: u8,
    ac_version: u8,
    ac_uid16: u16,
    ac_gid16: u16,
    ac_tty: u16,
    ac_btime: u32,
    ac_utime: u16,
    ac_stime: u16,
    ac_etime: u16,
    ac_mem: u16,
    ac_io: u16,
    ac_rw: u16,
    ac_minflt: u16,
    ac_majflt: u16,
    ac_swaps: u16,
    ac_ahz: u16,
    ac_exitcode: u32,
    ac_comm: [u8; ACCT_COMM + 1],
    ac_etime_hi: u8,
    ac_etime_lo: u16,
    ac_uid: u32,
    ac_gid: u32,
}

const _: () = assert!(core::mem::size_of::<LinuxAcct>() == 64);

pub(crate) fn sys_acct(args: SyscallArgs) -> UserRet {
    match do_acct(args.arg(0)) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn do_acct(path_ptr: usize) -> Result<(), ErrNo> {
    if cred::current_credentials().effective_uid.0 != 0 {
        return Err(ErrNo::EPERM);
    }
    if path_ptr == 0 {
        *ACCOUNTING_PATH.lock() = None;
        return Ok(());
    }

    let path = copy_user_path_cstr(path_ptr, 256)?;
    if path == "/dev/null" {
        return Err(ErrNo::EACCES);
    }
    let resolved = resolve_path_at(AT_FDCWD, path.as_str())?;
    let resolved = resolve_final_symlink(resolved.as_str())?;
    if path.ends_with('/') {
        match active_impl::backend().metadata(resolved.as_str()) {
            Ok(meta) if meta.node_type != VfsNodeType::Directory => return Err(ErrNo::ENOTDIR),
            Ok(_) => {}
            Err(VfsError::NotAFile) => return Err(ErrNo::ENOTDIR),
            Err(e) => return Err(vfs_error_to_errno(e)),
        }
    }

    if let Err(e) = vfs::assert_path_writable(resolved.as_str()) {
        return Err(vfs_error_to_errno(e));
    }

    match active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) => match meta.node_type {
            VfsNodeType::File => {
                *ACCOUNTING_PATH.lock() = Some(resolved);
                Ok(())
            }
            VfsNodeType::Directory => Err(ErrNo::EISDIR),
            VfsNodeType::Symlink => Err(ErrNo::ELOOP),
            VfsNodeType::Special => Err(ErrNo::EACCES),
        },
        Err(VfsError::NotAFile) => Err(ErrNo::ENOTDIR),
        Err(e) => Err(vfs_error_to_errno(e)),
    }
}

pub(crate) fn record_current_process_exit(exit_code: isize) {
    let path = match ACCOUNTING_PATH.lock().clone() {
        Some(path) => path,
        None => return,
    };
    let Some(process) = task::current_process_snapshot() else {
        return;
    };
    let leader = process.leader_task_id;
    let cred = cred::credentials_for(leader);
    let record = build_record(process.pid.raw(), leader, exit_code, &cred);
    if let Err(e) = append_record(path.as_str(), &record) {
        if e == ErrNo::ENOENT || e == ErrNo::ENOTDIR || e == ErrNo::EBADF {
            *ACCOUNTING_PATH.lock() = None;
        }
        log::warn!("[syscall] acct append {} failed: {:?}", path, e);
    }
}

fn build_record(
    pid: usize,
    leader: task::TaskId,
    exit_code: isize,
    cred: &cred::ProcessCredentials,
) -> LinuxAcct {
    let uid = cred.real_uid.0;
    let gid = cred.real_gid.0;
    let mut ac_comm = [0u8; ACCT_COMM + 1];
    let comm = command_name(pid, leader);
    let bytes = comm.as_bytes();
    let n = bytes.len().min(ACCT_COMM);
    ac_comm[..n].copy_from_slice(&bytes[..n]);

    LinuxAcct {
        ac_flag: 0,
        ac_version: ACCT_VERSION,
        ac_uid16: uid as u16,
        ac_gid16: gid as u16,
        ac_tty: 0,
        ac_btime: realtime_seconds(),
        ac_utime: 0,
        ac_stime: 0,
        ac_etime: 0,
        ac_mem: 0,
        ac_io: 0,
        ac_rw: 0,
        ac_minflt: 0,
        ac_majflt: 0,
        ac_swaps: 0,
        ac_ahz: ACCT_AHZ,
        ac_exitcode: wait_status_from_exit(exit_code),
        ac_comm,
        ac_etime_hi: 0,
        ac_etime_lo: 0,
        ac_uid: uid,
        ac_gid: gid,
    }
}

fn command_name(pid: usize, leader: task::TaskId) -> String {
    if let Some(argv) = vfs::cwd::lookup_argv_for_task(leader) {
        if let Some(arg0) = argv.first() {
            return basename(arg0.as_str());
        }
    }
    if let Some(exe) = vfs::cwd::lookup_exe_for_task(leader) {
        return basename(exe.as_str());
    }
    alloc::format!("{pid}")
}

fn basename(path: &str) -> String {
    String::from(path.rsplit('/').next().unwrap_or(path))
}

fn realtime_seconds() -> u32 {
    (platform::wall_clock::realtime_ns().unwrap_or(0) / 1_000_000_000).min(u32::MAX as u128) as u32
}

fn wait_status_from_exit(exit_code: isize) -> u32 {
    ((exit_code.max(0) as u32) & 0xff) << 8
}

fn append_record(path: &str, record: &LinuxAcct) -> Result<(), ErrNo> {
    let mut handle = active_impl::backend()
        .open(path, VfsOpenFlags(VfsOpenFlags::WRITE))
        .map_err(vfs_error_to_errno)?;
    let size = handle.metadata().map_err(vfs_error_to_errno)?.size;
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (record as *const LinuxAcct) as *const u8,
            core::mem::size_of::<LinuxAcct>(),
        )
    };
    let n = handle.write_at(size, bytes).map_err(vfs_error_to_errno)?;
    if n != bytes.len() {
        return Err(ErrNo::EIO);
    }
    handle.flush().map_err(vfs_error_to_errno)?;
    handle.close().map_err(vfs_error_to_errno)
}
