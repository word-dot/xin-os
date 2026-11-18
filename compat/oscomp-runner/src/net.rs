
//! Network syscall implementations.
//!
//! This module provides socket syscalls by wrapping axstd's networking API
//! (which is built on top of axnet/smoltcp) and managing socket file descriptors
//! in the process's fd table.

extern crate alloc;

use alloc::sync::Arc;
use axerrno::{AxError, AxResult};
use axstd::net::{TcpListener, TcpStream, UdpSocket, SocketAddr, Ipv4Addr, SocketAddrV4};
use axstd::sync::Mutex;
use memory_addr::VirtAddr;

use crate::process::{ProcessControlBlock, FdTable};

// ============================================================
// Socket types
// ============================================================

/// AF_INET constant
const AF_INET: usize = 2;

/// SOCK_STREAM constant
const SOCK_STREAM: usize = 1;

/// SOCK_DGRAM constant
const SOCK_DGRAM: usize = 2;

/// IPPROTO_TCP constant
const IPPROTO_TCP: usize = 6;

/// IPPROTO_UDP constant
const IPPROTO_UDP: usize = 17;

/// SHUT_RDWR constant
const SHUT_RDWR: usize = 2;

/// A socket that can be stored in the fd table.
pub enum Socket {
    TcpListener(Mutex<TcpListener>),
    TcpStream(Mutex<TcpStream>),
    Udp(Mutex<UdpSocket>),
}

impl Socket {
    /// Add this socket to the current process's fd table.
    pub fn add_to_fd_table(self) -> AxResult<usize> {
        let pcb = crate::task::current_pcb();
        let mut pcb_guard = pcb.lock();
        let mut fd_table = pcb_guard.fd_table.lock();
        let fd = fd_table.alloc_net_fd()
            .ok_or(AxError::WouldBlock)?;
        fd_table.set_socket(fd, Arc::new(self));
        Ok(fd)
    }

    /// Get a socket from the current process's fd table by fd.
    pub fn from_fd(fd: usize) -> AxResult<Arc<Socket>> {
        let pcb = crate::task::current_pcb();
        let pcb_guard = pcb.lock();
        let fd_table = pcb_guard.fd_table.lock();
        fd_table.get_socket(fd).cloned()
            .ok_or(AxError::BadFileDescriptor)
    }
}

// ============================================================
// sockaddr helpers
// ============================================================

/// C struct sockaddr_in layout (RISC-V 64-bit):
///   sin_family: u16 (2 bytes)
///   sin_port:   u16 (2 bytes, network byte order)
///   sin_addr:   u32 (4 bytes, network byte order)
///   sin_zero:   [u8; 8] (padding)
/// Total: 16 bytes

/// Read a SocketAddr from user memory at the given address.
fn read_sockaddr(addr: usize, addrlen: usize) -> AxResult<SocketAddr> {
    if addr == 0 || addrlen < 16 {
        return Err(AxError::InvalidInput);
    }

    let pcb = crate::task::current_pcb();
    let pcb_guard = pcb.lock();
    let aspace = pcb_guard.aspace.lock();

    let mut buf = [0u8; 16];
    aspace.read(VirtAddr::from_usize(addr), &mut buf)
        .map_err(|_| AxError::BadAddress)?;

    let family = u16::from_ne_bytes([buf[0], buf[1]]);
    if family != AF_INET as u16 {
        return Err(AxError::InvalidInput);
    }

    let port = u16::from_be_bytes([buf[2], buf[3]]);
    let ip = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let ip_addr = Ipv4Addr::from(ip);

    Ok(SocketAddr::V4(SocketAddrV4::new(ip_addr, port)))
}

/// Write a SocketAddr to user memory at the given address.
fn write_sockaddr(addr: usize, socket_addr: SocketAddr) -> AxResult {
    if addr == 0 {
        return Ok(());
    }

    let (port, ip) = match socket_addr {
        SocketAddr::V4(v4) => (v4.port(), u32::from(*v4.ip())),
        SocketAddr::V6(_) => return Err(AxError::InvalidInput),
    };

    let mut buf = [0u8; 16];
    let family = AF_INET as u16;
    buf[0..2].copy_from_slice(&family.to_ne_bytes());
    buf[2..4].copy_from_slice(&port.to_be_bytes());
    buf[4..8].copy_from_slice(&ip.to_be_bytes());
    // sin_zero is already zeroed

    let pcb = crate::task::current_pcb();
    let pcb_guard = pcb.lock();
    let aspace = pcb_guard.aspace.lock();
    aspace.write(VirtAddr::from_usize(addr), &buf)
        .map_err(|_| AxError::BadAddress)?;

    Ok(())
}

// ============================================================
// Network syscall implementations
// ============================================================

/// sys_socket: Create a socket for communication.
pub fn sys_socket(domain: usize, socktype: usize, protocol: usize) -> AxResult<isize> {
    axlog::debug!("sys_socket <= domain={}, socktype={}, protocol={}", domain, socktype, protocol);

    if domain != AF_INET {
        return Err(AxError::InvalidInput);
    }

    match (socktype, protocol) {
        (SOCK_STREAM, IPPROTO_TCP) | (SOCK_STREAM, 0) => {
            let listener = TcpListener::bind("0.0.0.0:0")
                .map_err(|_| AxError::ConnectionRefused)?;
            Socket::TcpListener(Mutex::new(listener)).add_to_fd_table()
                .map(|fd| fd as isize)
        }
        (SOCK_DGRAM, IPPROTO_UDP) | (SOCK_DGRAM, 0) => {
            let udp = UdpSocket::bind("0.0.0.0:0")
                .map_err(|_| AxError::ConnectionRefused)?;
            Socket::Udp(Mutex::new(udp)).add_to_fd_table()
                .map(|fd| fd as isize)
        }
        _ => Err(AxError::InvalidInput),
    }
}

/// sys_bind: Bind a socket to an address.
pub fn sys_bind(fd: usize, addr_ptr: usize, addrlen: usize) -> AxResult<isize> {
    axlog::debug!("sys_bind <= fd={}, addr_ptr={:#x}, addrlen={}", fd, addr_ptr, addrlen);
    let _addr = read_sockaddr(addr_ptr, addrlen)?;
    // For TCP: the socket is already bound when created via TcpListener::bind
    // For UDP: the socket is already bound when created via UdpSocket::bind
    // We could re-bind here if needed, but for now, just return success
    // since we pre-bind to 0.0.0.0:0 on creation.
    // TODO: implement actual re-bind if the address differs
    Ok(0)
}

/// sys_listen: Listen for connections on a socket.
pub fn sys_listen(fd: usize, backlog: usize) -> AxResult<isize> {
    axlog::debug!("sys_listen <= fd={}, backlog={}", fd, backlog);
    // TCP listener is already listening after TcpListener::bind
    // Just verify it's a TCP listener
    let socket = Socket::from_fd(fd)?;
    match &*socket {
        Socket::TcpListener(_) => Ok(0),
        _ => Err(AxError::InvalidInput),
    }
}

/// sys_accept: Accept a connection on a socket.
pub fn sys_accept(fd: usize, addr_ptr: usize, addrlen_ptr: usize) -> AxResult<isize> {
    axlog::debug!("sys_accept <= fd={}, addr_ptr={:#x}, addrlen_ptr={:#x}", fd, addr_ptr, addrlen_ptr);

    let socket = Socket::from_fd(fd)?;
    let listener = match &*socket {
        Socket::TcpListener(l) => l,
        _ => return Err(AxError::InvalidInput),
    };

    let (tcp_stream, peer_addr) = listener.lock().accept()
        .map_err(|_| AxError::ConnectionRefused)?;

    // Write peer address if requested
    if addr_ptr != 0 {
        write_sockaddr(addr_ptr, peer_addr)?;
        if addrlen_ptr != 0 {
            let pcb = crate::task::current_pcb();
            let pcb_guard = pcb.lock();
            let aspace = pcb_guard.aspace.lock();
            let len = 16u32;
            let _ = aspace.write(VirtAddr::from_usize(addrlen_ptr), &len.to_ne_bytes());
        }
    }

    Socket::TcpStream(Mutex::new(tcp_stream)).add_to_fd_table()
        .map(|fd| fd as isize)
}

/// sys_connect: Connect a socket to an address.
pub fn sys_connect(fd: usize, addr_ptr: usize, addrlen: usize) -> AxResult<isize> {
    axlog::debug!("sys_connect <= fd={}, addr_ptr={:#x}, addrlen={}", fd, addr_ptr, addrlen);
    let addr = read_sockaddr(addr_ptr, addrlen)?;

    let socket = Socket::from_fd(fd)?;
    match &*socket {
        Socket::TcpListener(_) => {
            // A listener can't connect; need to create a new stream
            let addr_str = format_addr(addr);
            let stream = TcpStream::connect(&*addr_str)
                .map_err(|_| AxError::ConnectionRefused)?;

            // Replace the listener with the stream in the fd table
            let pcb = crate::task::current_pcb();
            let mut pcb_guard = pcb.lock();
            let mut fd_table = pcb_guard.fd_table.lock();
            fd_table.set_socket(fd, Arc::new(Socket::TcpStream(Mutex::new(stream))));
            Ok(0)
        }
        Socket::TcpStream(s) => {
            // Already connected or need to connect
            // axstd TcpStream::connect already connects on creation
            // If we have a stream, it should already be connected
            Ok(0)
        }
        Socket::Udp(s) => {
            s.lock().connect(addr)
                .map_err(|_| AxError::ConnectionRefused)?;
            Ok(0)
        }
    }
}

/// sys_sendto: Send a message on a socket.
pub fn sys_sendto(
    fd: usize, buf_ptr: usize, len: usize, flags: usize,
    addr_ptr: usize, addrlen: usize,
) -> AxResult<isize> {
    axlog::debug!("sys_sendto <= fd={}, buf_ptr={:#x}, len={}, flags={}, addr_ptr={:#x}, addrlen={}",
        fd, buf_ptr, len, flags, addr_ptr, addrlen);

    if buf_ptr == 0 {
        return Err(AxError::BadAddress);
    }

    // Read buffer from user space
    let pcb = crate::task::current_pcb();
    let pcb_guard = pcb.lock();
    let aspace = pcb_guard.aspace.lock();
    let mut buf = alloc::vec![0u8; len];
    aspace.read(VirtAddr::from_usize(buf_ptr), &mut buf)
        .map_err(|_| AxError::BadAddress)?;
    drop(aspace);
    drop(pcb_guard);

    let socket = Socket::from_fd(fd)?;
    match &*socket {
        Socket::TcpStream(s) => {
            use axstd::io::Write;
            let mut stream = s.lock();
            match stream.write(&buf) {
                Ok(n) => Ok(n as isize),
                Err(_) => Err(AxError::ConnectionReset),
            }
        }
        Socket::Udp(s) => {
            if addr_ptr != 0 {
                let addr = read_sockaddr(addr_ptr, addrlen)?;
                let addr_str = format_addr(addr);
                s.lock().send_to(&buf, &*addr_str)
                    .map(|n| n as isize)
                    .map_err(|_| AxError::ConnectionReset)
            } else {
                // UDP connected: use send
                s.lock().send(&buf)
                    .map(|n| n as isize)
                    .map_err(|_| AxError::ConnectionReset)
            }
        }
        Socket::TcpListener(_) => Err(AxError::InvalidInput),
    }
}

/// sys_recvfrom: Receive a message from a socket.
pub fn sys_recvfrom(
    fd: usize, buf_ptr: usize, len: usize, flags: usize,
    addr_ptr: usize, addrlen_ptr: usize,
) -> AxResult<isize> {
    axlog::debug!("sys_recvfrom <= fd={}, buf_ptr={:#x}, len={}, flags={}, addr_ptr={:#x}, addrlen_ptr={:#x}",
        fd, buf_ptr, len, flags, addr_ptr, addrlen_ptr);

    if buf_ptr == 0 || len == 0 {
        return Err(AxError::BadAddress);
    }

    let socket = Socket::from_fd(fd)?;
    let mut recv_buf = alloc::vec![0u8; len];

    let (n, peer_addr) = match &*socket {
        Socket::TcpStream(s) => {
            use axstd::io::Read;
            let mut stream = s.lock();
            match stream.read(&mut recv_buf) {
                Ok(n) => {
                    let addr = stream.peer_addr().ok();
                    (n, addr)
                }
                Err(_) => return Err(AxError::ConnectionReset),
            }
        }
        Socket::Udp(s) => {
            let mut udp = s.lock();
            match udp.recv_from(&mut recv_buf) {
                Ok((n, addr)) => (n, Some(addr)),
                Err(_) => return Err(AxError::ConnectionReset),
            }
        }
        Socket::TcpListener(_) => return Err(AxError::InvalidInput),
    };

    // Write received data to user space
    let pcb = crate::task::current_pcb();
    let pcb_guard = pcb.lock();
    let aspace = pcb_guard.aspace.lock();
    aspace.write(VirtAddr::from_usize(buf_ptr), &recv_buf[..n])
        .map_err(|_| AxError::BadAddress)?;

    // Write peer address if requested
    if addr_ptr != 0 {
        if let Some(addr) = peer_addr {
            write_sockaddr(addr_ptr, addr)?;
            if addrlen_ptr != 0 {
                let len = 16u32;
                let _ = aspace.write(VirtAddr::from_usize(addrlen_ptr), &len.to_ne_bytes());
            }
        }
    }

    Ok(n as isize)
}

/// sys_send: Send a message on a connected socket.
pub fn sys_send(fd: usize, buf_ptr: usize, len: usize, flags: usize) -> AxResult<isize> {
    sys_sendto(fd, buf_ptr, len, flags, 0, 0)
}

/// sys_recv: Receive a message from a connected socket.
pub fn sys_recv(fd: usize, buf_ptr: usize, len: usize, flags: usize) -> AxResult<isize> {
    sys_recvfrom(fd, buf_ptr, len, flags, 0, 0)
}

/// sys_shutdown: Shut down part of a full-duplex connection.
pub fn sys_shutdown(fd: usize, how: usize) -> AxResult<isize> {
    axlog::debug!("sys_shutdown <= fd={}, how={}", fd, how);
    // For now, just close the socket
    let _ = Socket::from_fd(fd)?;
    Ok(0)
}

/// sys_getsockname: Get the local address of a socket.
pub fn sys_getsockname(fd: usize, addr_ptr: usize, addrlen_ptr: usize) -> AxResult<isize> {
    axlog::debug!("sys_getsockname <= fd={}, addr_ptr={:#x}, addrlen_ptr={:#x}", fd, addr_ptr, addrlen_ptr);

    let socket = Socket::from_fd(fd)?;
    let local_addr = match &*socket {
        Socket::TcpListener(s) => s.lock().local_addr().ok(),
        Socket::TcpStream(s) => s.lock().local_addr().ok(),
        Socket::Udp(s) => s.lock().local_addr().ok(),
    };

    if let Some(addr) = local_addr {
        if addr_ptr != 0 {
            write_sockaddr(addr_ptr, addr)?;
        }
        if addrlen_ptr != 0 {
            let pcb = crate::task::current_pcb();
            let pcb_guard = pcb.lock();
            let aspace = pcb_guard.aspace.lock();
            let len = 16u32;
            let _ = aspace.write(VirtAddr::from_usize(addrlen_ptr), &len.to_ne_bytes());
        }
    }

    Ok(0)
}

/// sys_getpeername: Get the peer address of a socket.
pub fn sys_getpeername(fd: usize, addr_ptr: usize, addrlen_ptr: usize) -> AxResult<isize> {
    axlog::debug!("sys_getpeername <= fd={}, addr_ptr={:#x}, addrlen_ptr={:#x}", fd, addr_ptr, addrlen_ptr);

    let socket = Socket::from_fd(fd)?;
    let peer_addr = match &*socket {
        Socket::TcpStream(s) => s.lock().peer_addr().ok(),
        Socket::Udp(s) => s.lock().peer_addr().ok(),
        _ => return Err(AxError::InvalidInput),
    };

    if let Some(addr) = peer_addr {
        if addr_ptr != 0 {
            write_sockaddr(addr_ptr, addr)?;
        }
        if addrlen_ptr != 0 {
            let pcb = crate::task::current_pcb();
            let pcb_guard = pcb.lock();
            let aspace = pcb_guard.aspace.lock();
            let len = 16u32;
            let _ = aspace.write(VirtAddr::from_usize(addrlen_ptr), &len.to_ne_bytes());
        }
    }

    Ok(0)
}

/// sys_setsockopt: Set options on a socket (stub).
pub fn sys_setsockopt(fd: usize, level: usize, optname: usize, optval_ptr: usize, optlen: usize) -> AxResult<isize> {
    axlog::debug!("sys_setsockopt <= fd={}, level={}, optname={}, optval_ptr={:#x}, optlen={}",
        fd, level, optname, optval_ptr, optlen);
    // Stub: most socket options are not critical for basic functionality
    Ok(0)
}

/// sys_getsockopt: Get options on a socket (stub).
pub fn sys_getsockopt(fd: usize, level: usize, optname: usize, optval_ptr: usize, optlen_ptr: usize) -> AxResult<isize> {
    axlog::debug!("sys_getsockopt <= fd={}, level={}, optname={}, optval_ptr={:#x}, optlen_ptr={:#x}",
        fd, level, optname, optval_ptr, optlen_ptr);
    // Stub: return default values for common options
    Ok(0)
}

// ============================================================
// Helpers
// ============================================================

/// Format a SocketAddr as a string for axstd's connect/bind calls.
fn format_addr(addr: SocketAddr) -> alloc::string::String {
    match addr {
        SocketAddr::V4(v4) => alloc::format!("{}:{}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => alloc::format!("[{}]:{}", v6.ip(), v6.port()),
    }
}
