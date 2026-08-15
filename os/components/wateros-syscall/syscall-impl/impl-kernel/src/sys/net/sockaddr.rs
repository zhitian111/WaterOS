//! Internet socket 地址在 Linux UAPI 与网络组件类型之间的转换。

use api_v0::ErrNo;
use network::{NetworkAddress, NetworkEndpoint, SocketDomain};

use crate::user_copy::{copy_from_user_struct, copy_to_user, copy_to_user_struct};

const AF_UNSPEC : u16 = 0;
pub(super) const AF_INET : u16 = 2;
pub(super) const AF_INET6 : u16 = 10;

#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn {
    sin_family : u16,
    sin_port : u16,
    sin_addr : [u8; 4],
    sin_zero : [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn6 {
    sin6_family : u16,
    sin6_port : u16,
    sin6_flowinfo : u32,
    sin6_addr : [u8; 16],
    sin6_scope_id : u32,
}

const _ : [(); 16] = [(); core::mem::size_of::<SockAddrIn>()];
const _ : [(); 28] = [(); core::mem::size_of::<SockAddrIn6>()];

pub(super) fn read_endpoint(addr_ptr : usize, addrlen : usize) -> Result<NetworkEndpoint, ErrNo> {
    if addr_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if addrlen < core::mem::size_of::<u16>() {
        return Err(ErrNo::EINVAL);
    }
    let family = copy_from_user_struct::<u16>(addr_ptr)?;
    match family {
        AF_INET => {
            if addrlen < core::mem::size_of::<SockAddrIn>() {
                return Err(ErrNo::EINVAL);
            }
            let address = copy_from_user_struct::<SockAddrIn>(addr_ptr)?;
            Ok(NetworkEndpoint { address : NetworkAddress::Ipv4(address.sin_addr),
                                 port : u16::from_be(address.sin_port),
                                 scope_id : 0 })
        }
        AF_INET6 => {
            if addrlen < core::mem::size_of::<SockAddrIn6>() {
                return Err(ErrNo::EINVAL);
            }
            let address = copy_from_user_struct::<SockAddrIn6>(addr_ptr)?;
            Ok(NetworkEndpoint { address : NetworkAddress::Ipv6(address.sin6_addr),
                                 port : u16::from_be(address.sin6_port),
                                 scope_id : address.sin6_scope_id })
        }
        _ => Err(ErrNo::EAFNOSUPPORT),
    }
}

/// 解析 bind(2) 地址。旧 IPv4 实现允许 sockaddr_in 的 family 为
/// AF_UNSPEC，因此在通用 sockaddr 解析层继续保留这一兼容行为。
pub(super) fn read_bind_endpoint(addr_ptr : usize,
                                 addrlen : usize)
                                 -> Result<NetworkEndpoint, ErrNo> {
    if addr_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if addrlen < core::mem::size_of::<u16>() {
        return Err(ErrNo::EINVAL);
    }
    if copy_from_user_struct::<u16>(addr_ptr)? != AF_UNSPEC {
        return read_endpoint(addr_ptr, addrlen);
    }
    if addrlen < core::mem::size_of::<SockAddrIn>() {
        return Err(ErrNo::EINVAL);
    }
    let address = copy_from_user_struct::<SockAddrIn>(addr_ptr)?;
    Ok(NetworkEndpoint { address : NetworkAddress::Ipv4(address.sin_addr),
                         port : u16::from_be(address.sin_port),
                         scope_id : 0 })
}

pub(super) fn endpoint_domain(endpoint : NetworkEndpoint) -> SocketDomain {
    endpoint.address
            .domain()
}

pub(super) fn endpoint_size(endpoint : NetworkEndpoint) -> usize {
    match endpoint.address {
        NetworkAddress::Ipv4(_) => core::mem::size_of::<SockAddrIn>(),
        NetworkAddress::Ipv6(_) => core::mem::size_of::<SockAddrIn6>(),
    }
}

pub(super) fn copy_endpoint_to_user(endpoint : NetworkEndpoint,
                                    addr_ptr : usize,
                                    capacity : usize)
                                    -> Result<usize, ErrNo> {
    let actual = endpoint_size(endpoint);
    let write_len = actual.min(capacity);
    if write_len == 0 {
        return Ok(actual);
    }
    if addr_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    match endpoint.address {
        NetworkAddress::Ipv4(address) => {
            let sockaddr = SockAddrIn { sin_family : AF_INET,
                                        sin_port : endpoint.port
                                                           .to_be(),
                                        sin_addr : address,
                                        sin_zero : [0; 8] };
            let bytes = unsafe {
                core::slice::from_raw_parts(&sockaddr as *const SockAddrIn as *const u8,
                                            write_len)
            };
            copy_to_user(addr_ptr, bytes)?;
        }
        NetworkAddress::Ipv6(address) => {
            let sockaddr = SockAddrIn6 { sin6_family : AF_INET6,
                                         sin6_port : endpoint.port
                                                             .to_be(),
                                         sin6_flowinfo : 0,
                                         sin6_addr : address,
                                         sin6_scope_id : endpoint.scope_id };
            let bytes = unsafe {
                core::slice::from_raw_parts(&sockaddr as *const SockAddrIn6 as *const u8,
                                            write_len)
            };
            copy_to_user(addr_ptr, bytes)?;
        }
    }
    Ok(actual)
}

pub(super) fn write_endpoint(endpoint : NetworkEndpoint,
                             addr_ptr : usize,
                             addrlen_ptr : usize)
                             -> Result<(), ErrNo> {
    if addr_ptr == 0 || addrlen_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let capacity = copy_from_user_struct::<u32>(addrlen_ptr)? as usize;
    if capacity > i32::MAX as usize {
        return Err(ErrNo::EINVAL);
    }
    let actual = copy_endpoint_to_user(endpoint, addr_ptr, capacity)?;
    copy_to_user_struct(addrlen_ptr, &(actual as u32))
}

#[cfg(test)]
mod tests {
    use super::{SockAddrIn, SockAddrIn6};

    #[test]
    fn linux_sockaddr_sizes_match() {
        assert_eq!(core::mem::size_of::<SockAddrIn>(), 16);
        assert_eq!(core::mem::size_of::<SockAddrIn6>(), 28);
    }
}
