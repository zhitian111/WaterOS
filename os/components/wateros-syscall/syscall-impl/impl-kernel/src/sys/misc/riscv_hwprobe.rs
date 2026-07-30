//! Conservative implementation of the RISC-V hardware probing ABI.

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user_struct};

const KEY_MVENDORID: i64 = 0;
const KEY_MARCHID: i64 = 1;
const KEY_MIMPID: i64 = 2;
const KEY_BASE_BEHAVIOR: i64 = 3;
const KEY_IMA_EXT_0: i64 = 4;
const KEY_CPUPERF_0: i64 = 5;
const KEY_ZICBOZ_BLOCK_SIZE: i64 = 6;
const KEY_HIGHEST_VIRT_ADDRESS: i64 = 7;
const KEY_TIME_CSR_FREQ: i64 = 8;

const BASE_BEHAVIOR_IMA: u64 = 1 << 0;
const IMA_FD: u64 = 1 << 0;
const IMA_C: u64 = 1 << 1;
const MISALIGNED_SLOW: u64 = 2;
const SV39_USER_MAX: u64 = (1u64 << 38) - 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct HwprobePair {
    key: i64,
    value: u64,
}

pub(crate) fn sys_riscv_hwprobe(args: SyscallArgs) -> UserRet {
    match do_riscv_hwprobe(
        args.arg(0),
        args.arg(1),
        args.arg(2),
        args.arg(3),
        args.arg(4) as u32,
    ) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

fn do_riscv_hwprobe(
    pairs_ptr: usize,
    pair_count: usize,
    cpuset_size: usize,
    cpus_ptr: usize,
    flags: u32,
) -> Result<(), ErrNo> {
    if flags != 0 {
        return Err(ErrNo::EINVAL);
    }
    validate_cpu_set(cpuset_size, cpus_ptr)?;
    if pair_count != 0 && pairs_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }

    for index in 0..pair_count {
        let offset = index
            .checked_mul(core::mem::size_of::<HwprobePair>())
            .ok_or(ErrNo::EFAULT)?;
        let pair_ptr = pairs_ptr.checked_add(offset).ok_or(ErrNo::EFAULT)?;
        let mut pair: HwprobePair = copy_from_user_struct(pair_ptr)?;
        fill_pair(&mut pair);
        copy_to_user_struct(pair_ptr, &pair)?;
    }
    Ok(())
}

fn validate_cpu_set(cpuset_size: usize, cpus_ptr: usize) -> Result<(), ErrNo> {
    if cpuset_size == 0 && cpus_ptr == 0 {
        return Ok(());
    }
    if cpuset_size == 0 || cpus_ptr == 0 {
        return Err(ErrNo::EINVAL);
    }

    let mut bytes = [0u8; core::mem::size_of::<u64>()];
    let copied_len = cpuset_size.min(bytes.len());
    copy_from_user(&mut bytes[..copied_len], cpus_ptr)?;
    let requested = u64::from_le_bytes(bytes);
    if requested & task::online_cpu_mask().bits() == 0 {
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}

fn fill_pair(pair: &mut HwprobePair) {
    pair.value = match pair.key {
        KEY_MVENDORID | KEY_MARCHID | KEY_MIMPID => 0,
        KEY_BASE_BEHAVIOR => BASE_BEHAVIOR_IMA,
        KEY_IMA_EXT_0 => IMA_FD | IMA_C,
        KEY_CPUPERF_0 => MISALIGNED_SLOW,
        KEY_ZICBOZ_BLOCK_SIZE => 0,
        KEY_HIGHEST_VIRT_ADDRESS => SV39_USER_MAX,
        KEY_TIME_CSR_FREQ => platform::time::frequency_hz().unwrap_or(0),
        _ => {
            pair.key = -1;
            0
        }
    };
}
