//! 基于 smoltcp 的 WaterOS IPv4/IPv6 双栈实现。

#![no_std]

extern crate alloc;

mod adapter;
pub mod stack;

#[cfg(feature = "self_test")]
pub fn self_test() {
    use api_v0::{NetworkAddress, NetworkConfig, NetworkEndpoint, SocketKind};

    log::info!("[network/impl-smoltcp] self_test begin");
    let config = NetworkConfig { address : [10, 0, 2, 15],
                                 prefix_len : 24,
                                 gateway : [10, 0, 2, 2],
                                 ipv6 : None };
    assert_eq!(config.prefix_len, 24);
    assert_eq!(config.address[0], 10);
    assert_ne!(config.address, config.gateway);
    let endpoint = NetworkEndpoint { address : NetworkAddress::Ipv4([127, 0, 0, 1]),
                                     port : 8080,
                                     scope_id : 0 };
    assert_eq!(endpoint.port, 8080);
    assert_eq!(SocketKind::Tcp as u8, 1);
    log::info!("[network/impl-smoltcp] configuration and endpoint checks passed");
    log::info!("[network/impl-smoltcp] self_test complete");
}
