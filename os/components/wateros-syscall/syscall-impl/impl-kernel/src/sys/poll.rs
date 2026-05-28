//! `poll` — 遍历用户传入的 `struct pollfd` 数组，检查每个 fd 的读写就绪状态。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLHUP: i16 = 0x010;

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

pub(crate) fn sys_poll(args: SyscallArgs) -> UserRet {
    let fds_ptr = args.arg(0);
    let nfds = args.arg(1);
    // arg(2) = timeout，当前忽略（非阻塞单次检查）

    if nfds == 0 || fds_ptr == 0 {
        return UserRet::from_success(0);
    }
    if nfds > 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let pollfd_size = core::mem::size_of::<PollFd>();
    let mut ready_count: usize = 0;

    for i in 0..nfds {
        let ptr = fds_ptr + i * pollfd_size;
        let mut pfd: PollFd = match copy_from_user_struct(ptr) {
            Ok(v) => v,
            Err(_) => return UserRet::from_error(ErrNo::EFAULT),
        };

        if pfd.fd < 0 {
            continue;
        }

        let fd = pfd.fd as usize;

        // 检查是否是 socket fd
        if let Some(handle) = socket_fd::lookup(fd) {
            let kind = stack::socket_kind(handle);
            let state = stack::socket_state(handle);

            match kind {
                Ok(stack::SocketKind::Tcp) => {
                    match state {
                        Ok(stack::SocketState::Listening { .. }) => {
                            // 监听 socket：有入连接即 POLLIN
                            if pfd.events & POLLIN != 0 {
                                if stack::socket_has_pending_accept(handle).unwrap_or(false) {
                                    pfd.revents |= POLLIN;
                                }
                            }
                        }
                        Ok(stack::SocketState::Connecting) | Ok(stack::SocketState::Connected) => {
                            // 已连接或正在建立：检查收发就绪
                            if pfd.events & POLLIN != 0 {
                                if stack::socket_may_recv(handle).unwrap_or(false) {
                                    pfd.revents |= POLLIN;
                                }
                            }
                            if pfd.events & POLLOUT != 0 {
                                if stack::socket_may_send(handle).unwrap_or(false) {
                                    pfd.revents |= POLLOUT;
                                }
                            }
                            // 连接关闭
                            if stack::socket_is_connected(handle).unwrap_or(true) == false {
                                pfd.revents |= POLLHUP;
                            }
                        }
                        Ok(stack::SocketState::Closed) => {
                            pfd.revents |= POLLHUP;
                        }
                        _ => {}
                    }
                }
                Ok(stack::SocketKind::Udp) => {
                    // UDP 总是可写
                    if pfd.events & POLLOUT != 0 {
                        pfd.revents |= POLLOUT;
                    }
                    if pfd.events & POLLIN != 0 {
                        if stack::socket_udp_can_recv(handle).unwrap_or(false) {
                            pfd.revents |= POLLIN;
                        }
                    }
                }
                _ => {}
            }
        }
        // 非 socket fd（如 pipe、文件）：暂不做 poll 检查，留给后续扩展

        if pfd.revents != 0 {
            ready_count += 1;
            let _ = copy_to_user_struct(ptr, &pfd);
        }
    }

    UserRet::from_success(ready_count)
}
