//! `recvfrom(2)`：接收 TCP/UDP 数据。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::{stack, SocketKind, SocketReceiveLease, SocketRecvError, SocketRecvFinish};

use crate::fallible_buf::SYSCALL_IO_MAX;
use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;
use crate::user_copy::{copy_to_user_progress, copy_to_user_struct};

use super::sockaddr::copy_endpoint_to_user;

const MSG_DONTWAIT : usize = 0x40;
const MSG_PEEK : usize = 0x02;
const MSG_TRUNC : usize = 0x20;
const MSG_OOB : usize = 0x01;
const MSG_ERRQUEUE : usize = 0x2000;

// 本方法代码由AI完成
pub(crate) fn sys_recvfrom(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let buf_ptr = args.arg(1);
    let len = args.arg(2);
    let flags = args.arg(3);
    let addr_ptr = args.arg(4);
    let addrlen_ptr = args.arg(5);

    if len == 0 {
        return UserRet::from_success(0);
    }
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags & MSG_OOB != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & MSG_ERRQUEUE != 0 {
        return UserRet::from_error(ErrNo::EAGAIN);
    }

    if crate::unix_sock::is_unix_fd(fd) {
        return match crate::unix_sock::recvfrom_unix(fd,
                                                     buf_ptr,
                                                     len.min(SYSCALL_IO_MAX),
                                                     addr_ptr,
                                                     addrlen_ptr)
        {
            Ok(n) => UserRet::from_success(n),
            Err(e) => UserRet::from_error(e),
        };
    }

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(socket) => socket,
        Err(error) => return UserRet::from_error(error),
    };
    let kind = match socket.kind() {
        Ok(kind) => kind,
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    };
    let source_capacity = match source_address_capacity(addr_ptr, addrlen_ptr) {
        Ok(capacity) => capacity,
        Err(error) => return UserRet::from_error(error),
    };
    let nonblocking = socket_fd::is_nonblocking(fd) || flags & MSG_DONTWAIT != 0;
    let lease = match receive_blocking(&socket,
                                       len.min(SYSCALL_IO_MAX),
                                       nonblocking)
    {
        Ok(Some(lease)) => lease,
        Ok(None) => return UserRet::from_success(0),
        Err(error) => return UserRet::from_error(error),
    };
    let datagram_len = lease.datagram_len();
    let progress = copy_to_user_progress(buf_ptr, lease.bytes());

    if matches!(kind, SocketKind::Udp | SocketKind::Icmp) &&
       progress.error
               .is_none()
    {
        let source = lease.source();
        if let Err(error) = write_source_address(source,
                                                 addr_ptr,
                                                 addrlen_ptr,
                                                 source_capacity)
        {
            let _ = lease.finish(0, false);
            return UserRet::from_error(error);
        }
    }

    if flags & MSG_PEEK != 0 {
        let _ = lease.finish(0, false);
        return if progress.copied > 0 ||
                  progress.error
                          .is_none()
        {
            let returned = if matches!(kind, SocketKind::Udp | SocketKind::Icmp) &&
                              flags & MSG_TRUNC != 0 {
                datagram_len
            } else {
                progress.copied
            };
            UserRet::from_success(returned)
        } else {
            UserRet::from_error(ErrNo::EFAULT)
        };
    }

    match lease.finish(progress.copied,
                       progress.error
                               .is_none())
    {
        Ok(SocketRecvFinish::Bytes(copied)) => {
            if matches!(kind, SocketKind::Udp | SocketKind::Icmp) && flags & MSG_TRUNC != 0 {
                UserRet::from_success(datagram_len)
            } else {
                UserRet::from_success(copied)
            }
        }
        Ok(SocketRecvFinish::Fault) => UserRet::from_error(ErrNo::EFAULT),
        Err(error) => UserRet::from_error(recv_error_to_errno(error)),
    }
}

fn receive_blocking(socket : &network::SocketRef,
                    max_len : usize,
                    nonblocking : bool)
                    -> Result<Option<SocketReceiveLease>, ErrNo> {
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        drive_network_stack();
        match socket.prepare_receive(max_len) {
            Ok(lease) => return Ok(Some(lease)),
            Err(SocketRecvError::Finished) => return Ok(None),
            Err(SocketRecvError::Busy | SocketRecvError::Empty) => {}
            Err(error) => return Err(recv_error_to_errno(error)),
        }
        socket_blocking_tick(nonblocking, task_id)?;
    }
}

fn write_source_address(endpoint : network::NetworkEndpoint,
                        addr_ptr : usize,
                        addrlen_ptr : usize,
                        capacity : Option<u32>)
                        -> Result<(), ErrNo> {
    let Some(addrlen) = capacity else {
        return Ok(());
    };
    let actual = copy_endpoint_to_user(endpoint, addr_ptr, addrlen as usize)?;
    copy_to_user_struct(addrlen_ptr, &(actual as u32)).map_err(|_| ErrNo::EFAULT)?;
    Ok(())
}

fn source_address_capacity(addr_ptr : usize, addrlen_ptr : usize) -> Result<Option<u32>, ErrNo> {
    if addr_ptr == 0 {
        return Ok(None);
    }
    if addrlen_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let addrlen =
        crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr).map_err(|_| ErrNo::EFAULT)?;
    if addrlen > i32::MAX as u32 {
        return Err(ErrNo::EINVAL);
    }
    Ok(Some(addrlen))
}

fn recv_error_to_errno(error : SocketRecvError) -> ErrNo {
    match error {
        SocketRecvError::Busy | SocketRecvError::Empty => ErrNo::EAGAIN,
        SocketRecvError::Finished => ErrNo::EIO,
        SocketRecvError::InvalidSocket => ErrNo::ENOTSOCK,
        SocketRecvError::NoMemory => ErrNo::ENOMEM,
        SocketRecvError::Io => ErrNo::EIO,
    }
}

fn drive_network_stack() {
    match platform::timer::now_duration() {
        Ok(now) => {
            let millis = now.as_millis()
                            .min(i64::MAX as u128) as i64;
            stack::poll_at_millis(millis);
        }
        Err(_) => stack::poll(),
    }
    stack::poll_socket_events();
}
