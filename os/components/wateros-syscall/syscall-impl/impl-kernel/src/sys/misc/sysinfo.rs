//! 系统信息类系统调用：`uname`、`sysinfo`、`getrandom`。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::{copy_to_user, copy_to_user_struct};

const UTS_LEN: usize = 65;
const GRND_NONBLOCK: usize = 0x0001;
const GRND_RANDOM: usize = 0x0002;
const GRND_INSECURE: usize = 0x0004;
const GETRANDOM_ALLOWED_FLAGS: usize = GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE;
const SYSINFO_TOTAL_RAM: usize = wateros_base_config::mm::QEMU_VIRT_PHYS_RAM_SIZE;
const SYSINFO_FREE_RAM: usize = SYSINFO_TOTAL_RAM / 2;

/// Linux `struct utsname`（与 libc 对齐）。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserUtsName {
    sysname: [u8; UTS_LEN],
    nodename: [u8; UTS_LEN],
    release: [u8; UTS_LEN],
    version: [u8; UTS_LEN],
    machine: [u8; UTS_LEN],
    domainname: [u8; UTS_LEN],
}

/// Linux LP64 `struct sysinfo` 布局。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserSysInfo {
    uptime: isize,
    loads: [usize; 3],
    totalram: usize,
    freeram: usize,
    sharedram: usize,
    bufferram: usize,
    totalswap: usize,
    freeswap: usize,
    procs: u16,
    pad: u16,
    totalhigh: usize,
    freehigh: usize,
    mem_unit: u32,
}

const _: () = assert!(core::mem::size_of::<UserSysInfo>() == 112);

fn make_uts_field(s: &str) -> [u8; UTS_LEN] {
    let mut buf = [0u8; UTS_LEN];
    let bytes = s.as_bytes();
    let n = bytes.len().min(UTS_LEN - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    buf
}

/// `uname(buf)` — 返回系统信息。
pub(crate) fn sys_uname(args: SyscallArgs) -> UserRet {
    let buf_ptr = args.arg(0);
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    #[cfg(target_arch = "riscv64")]
    let machine = "riscv64";
    #[cfg(target_arch = "loongarch64")]
    let machine = "loongarch64";
    #[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
    let machine = "unknown";
    let uts = UserUtsName {
        sysname: make_uts_field("WaterOS"),
        nodename: make_uts_field("wateros"),
        release: make_uts_field("5.15.0"),
        version: make_uts_field("WaterOS #1 SMP"),
        machine: make_uts_field(machine),
        domainname: make_uts_field(""),
    };
    match copy_to_user_struct(buf_ptr, &uts) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

pub(crate) fn sys_sysinfo(args: SyscallArgs) -> UserRet {
    let info_ptr = args.arg(0);
    if info_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let info = UserSysInfo {
        uptime: task::current_tick() as isize,
        loads: [0; 3],
        totalram: SYSINFO_TOTAL_RAM,
        freeram: SYSINFO_FREE_RAM,
        sharedram: 0,
        bufferram: 0,
        totalswap: 0,
        freeswap: 0,
        procs: 1,
        pad: 0,
        totalhigh: 0,
        freehigh: 0,
        mem_unit: 1,
    };
    match copy_to_user_struct(info_ptr, &info) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn random_seed(buf_ptr: usize, buflen: usize, flags: usize, tid: usize) -> u64 {
    let tick = task::current_tick() as u64;
    let mixed = (buf_ptr as u64).rotate_left(17)
        ^ (buflen as u64).rotate_left(31)
        ^ (flags as u64).rotate_left(7)
        ^ (tid as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ tick.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    if mixed == 0 {
        0x6A09_E667_F3BC_C909
    } else {
        mixed
    }
}

fn fill_pseudo_random(state: &mut u64, out: &mut [u8]) {
    for byte in out {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        *byte = (x >> 24) as u8;
    }
}

pub(crate) fn sys_getrandom(args: SyscallArgs) -> UserRet {
    let buf_ptr = args.arg(0);
    let buflen = args.arg(1);
    let flags = args.arg(2);

    if flags & !GETRANDOM_ALLOWED_FLAGS != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if buflen == 0 {
        return UserRet::from_success(0);
    }
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let tid = task::current_task_id().unwrap_or(0);
    let mut state = random_seed(buf_ptr, buflen, flags, tid);
    let mut written = 0usize;
    let mut chunk = [0u8; 64];
    while written < buflen {
        let n = core::cmp::min(chunk.len(), buflen - written);
        fill_pseudo_random(&mut state, &mut chunk[..n]);
        match copy_to_user(buf_ptr + written, &chunk[..n]) {
            Ok(copied) if copied == n => written += n,
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
    }

    UserRet::from_success(written)
}
