//! 网络接口、IPv4 地址与路由初始化。

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address};

use crate::adapter::SmoltcpAdapter;
use driver_network::first_network_device;

use super::state::{NetworkStack, NETWORK_STACK};
use super::types::{NetworkConfig, NetworkError};

/// 创建 smoltcp 协议栈并配置 IP；无真实网卡时仍启用 loopback-only 模式。
pub fn init(network_config : NetworkConfig) -> Result<(), NetworkError> {
    if network_config.prefix_len > 32 {
        return Err(NetworkError::InvalidArgument);
    }
    let ip = network_config.address;
    let gateway = network_config.gateway;
    let prefix_len = network_config.prefix_len;
    let mut adapter = match first_network_device() {
        Some(device) => SmoltcpAdapter::new(device),
        None => {
            log::warn!("[network-stack] no network device registered; using loopback-only mode");
            SmoltcpAdapter::loopback_only()
        }
    };
    let mac = adapter.mac_address();
    adapter.set_local_ipv4(ip);

    let config = Config::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
    let mut iface = Interface::new(config, &mut adapter, Instant::ZERO);

    iface.update_ip_addrs(|addrs| {
             addrs.push(IpCidr::new(Ipv4Address::new(ip[0], ip[1], ip[2], ip[3]).into(),
                                    prefix_len))
                  .unwrap();
             // loopback 地址：iperf / netperf / libc-test 均使用 127.0.0.1
             addrs.push(IpCidr::new(Ipv4Address::new(127, 0, 0, 1).into(), 8))
                  .unwrap();
         });
    // 默认路由：所有外部流量经网关
    iface.routes_mut()
         .add_default_ipv4_route(Ipv4Address::new(gateway[0], gateway[1], gateway[2], gateway[3]))
         .unwrap();
    // 添加本地子网路由（直接可达，无需网关）和 loopback 路由
    iface.routes_mut()
         .update(|storage| {
             // 本地子网 10.0.2.0/24 → 直接连接
             let _ = storage.push(smoltcp::iface::Route {
            cidr: smoltcp::wire::IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                Ipv4Address::new(ip[0], ip[1], ip[2], ip[3]),
                prefix_len,
            )),
            via_router: Ipv4Address::UNSPECIFIED.into(),
            preferred_until: None,
            expires_at: None,
        });
             // loopback 子网 127.0.0.0/8 → 本地
             let _ = storage.push(smoltcp::iface::Route {
            cidr: smoltcp::wire::IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                Ipv4Address::new(127, 0, 0, 1),
                8,
            )),
            via_router: Ipv4Address::UNSPECIFIED.into(),
            preferred_until: None,
            expires_at: None,
        });
         });

    let mut stack_slot = NETWORK_STACK.lock();
    if stack_slot.is_some() {
        return Err(NetworkError::AlreadyInitialized);
    }
    *stack_slot = Some(NetworkStack { adapter,
                                      iface,
                                      sockets : SocketSet::new(vec![]),
                                      metas : BTreeMap::new(),
                                      tcp_listener_groups : BTreeMap::new(),
                                      tcp_close_pending : BTreeSet::new(),
                                      udp_loopback : BTreeMap::new(),
                                      local_ip : ip,
                                      ephemeral_port : 49152,
                                      next_listener_group : 1 });
    drop(stack_slot);

    log::info!("[network-stack] initialized ip={}.{}.{}.{}/{} gateway={}.{}.{}.{}",
               ip[0],
               ip[1],
               ip[2],
               ip[3],
               prefix_len,
               gateway[0],
               gateway[1],
               gateway[2],
               gateway[3],);
    Ok(())
}
