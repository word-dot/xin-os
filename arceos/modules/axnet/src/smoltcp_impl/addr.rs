use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use smoltcp::wire::{IpAddress, IpEndpoint};

pub const UNSPECIFIED_IP: IpAddress = IpAddress::v4(0, 0, 0, 0);
pub const UNSPECIFIED_ENDPOINT: IpEndpoint = IpEndpoint::new(UNSPECIFIED_IP, 0);

/// Convert an [`IpEndpoint`] to a [`SocketAddr`].
/// This replaces the `From<IpEndpoint> for SocketAddr` impl that existed
/// in the rcore-os fork of smoltcp but is absent from the crates.io version.
pub fn endpoint_to_sockaddr(ep: IpEndpoint) -> SocketAddr {
    match ep.addr {
        IpAddress::Ipv4(v4) => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(v4), ep.port)),
        IpAddress::Ipv6(v6) => SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::from(v6), ep.port, 0, 0)),
    }
}

/// Convert a [`SocketAddr`] to an [`IpEndpoint`].
/// This replaces the `From<SocketAddr> for IpEndpoint` impl that existed
/// in the rcore-os fork of smoltcp but is absent from the crates.io version.
pub fn sockaddr_to_endpoint(addr: SocketAddr) -> IpEndpoint {
    match addr {
        SocketAddr::V4(v4) => IpEndpoint::new(IpAddress::Ipv4(v4.ip().octets().into()), v4.port()),
        SocketAddr::V6(v6) => IpEndpoint::new(IpAddress::Ipv6(v6.ip().octets().into()), v6.port()),
    }
}
