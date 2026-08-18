//! 系统信息类系统调用：`uname`、`sysinfo`、`getrandom`。

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use spin::Mutex;

use crate::user_copy::{copy_from_user, copy_to_user, copy_to_user_struct};

const UTS_LEN: usize = 65;
const GRND_NONBLOCK: usize = 0x0001;
const GRND_RANDOM: usize = 0x0002;
const GRND_INSECURE: usize = 0x0004;
const GETRANDOM_ALLOWED_FLAGS: usize = GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE;
const UTS_VALUE_MAX: usize = UTS_LEN - 1;

#[derive(Clone, Copy)]
struct UtsIdentity {
    /// 主机名定长缓冲（NUL 终止）。
    nodename: [u8; UTS_LEN],
    /// NIS 域名定长缓冲（NUL 终止）。
    domainname: [u8; UTS_LEN],
}

/// 主机名和 NIS 域名属于全局 UTS 状态；锁只保护两个定长数组，不跨用户拷贝。
static UTS_IDENTITY: Mutex<UtsIdentity> = Mutex::new(UtsIdentity {
    nodename: make_const_uts_field(b"wateros"),
    domainname: make_const_uts_field(b""),
});

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
    /// 系统启动以来的秒数。
    uptime: isize,
    /// 1、5、15 分钟负载定点值。
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

const fn make_const_uts_field(bytes: &[u8]) -> [u8; UTS_LEN] {
    let mut buf = [0u8; UTS_LEN];
    let mut index = 0;
    while index < bytes.len() && index < UTS_VALUE_MAX {
        buf[index] = bytes[index];
        index += 1;
    }
    buf
}

fn make_uts_field(s: &str) -> [u8; UTS_LEN] {
    make_const_uts_field(s.as_bytes())
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    let short = make_const_uts_field(b"wateros");
    assert_eq!(&short[..8], b"wateros\0");
    let long = make_const_uts_field(&[b'x'; UTS_LEN + 8]);
    assert_eq!(long[UTS_LEN - 1], 0);
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
    let identity = *UTS_IDENTITY.lock();
    let uts = UserUtsName {
        // 用户态按 Linux syscall ABI 运行，许多构建脚本、LTP 与运行库会用
        // uname.sysname 选择代码路径；内核品牌保留在 version 字段。
        sysname: make_uts_field("Linux"),
        nodename: identity.nodename,
        release: make_uts_field("5.15.0"),
        version: make_uts_field("WaterOS #1 SMP"),
        machine: make_uts_field(machine),
        domainname: identity.domainname,
    };
    match copy_to_user_struct(buf_ptr, &uts) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn set_uts_value(user_ptr: usize, len: usize, domain: bool) -> Result<(), ErrNo> {
    if cred::current_credentials().effective_uid.0 != 0 {
        return Err(ErrNo::EPERM);
    }
    if len > UTS_VALUE_MAX {
        return Err(ErrNo::EINVAL);
    }

    // 长度为零时 Linux 不读取用户指针，因此空主机名允许传入 NULL。
    let mut value = [0u8; UTS_LEN];
    if len != 0 {
        let copied = copy_from_user(&mut value[..len], user_ptr)?;
        if copied != len {
            return Err(ErrNo::EFAULT);
        }
    }

    let mut identity = UTS_IDENTITY.lock();
    if domain {
        identity.domainname = value;
    } else {
        identity.nodename = value;
    }
    Ok(())
}

/// `sethostname(name, len)` — 修改 `uname().nodename`。
pub(crate) fn sys_sethostname(args: SyscallArgs) -> UserRet {
    match set_uts_value(args.arg(0), args.arg(1), false) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

/// `setdomainname(name, len)` — 修改 `uname().domainname`。
pub(crate) fn sys_setdomainname(args: SyscallArgs) -> UserRet {
    match set_uts_value(args.arg(0), args.arg(1), true) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

pub(crate) fn sys_sysinfo(args: SyscallArgs) -> UserRet {
    let info_ptr = args.arg(0);
    if info_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let memory = mm::frame_alloctor::frame_mem_stats();
    let totalram = usize::try_from(memory.total_bytes()).unwrap_or(usize::MAX);
    let freeram = usize::try_from(memory.free_bytes()).unwrap_or(usize::MAX);
    let process_count = task::all_process_pids().len().min(u16::MAX as usize) as u16;
    let info = UserSysInfo {
        uptime: platform::timer::now_duration()
                    .map(|duration| duration.as_secs() as isize)
                    .unwrap_or(0),
        loads: [0; 3],
        totalram,
        freeram,
        sharedram: 0,
        bufferram: 0,
        totalswap: 0,
        freeswap: 0,
        procs: process_count,
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

#[cfg(test)]
mod tests {
    use super::{make_const_uts_field, UTS_LEN};

    #[test]
    fn uts_field_is_nul_terminated_and_truncated() {
        let short = make_const_uts_field(b"wateros");
        assert_eq!(&short[..8], b"wateros\0");

        let long = make_const_uts_field(&[b'x'; UTS_LEN + 8]);
        assert_eq!(long[UTS_LEN - 2], b'x');
        assert_eq!(long[UTS_LEN - 1], 0);
    }
}
