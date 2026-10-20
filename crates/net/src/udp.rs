use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

use crate::Transport;
use crate::frame::MAX_DATAGRAM;

/// Plain non-blocking UDP: localhost for development, direct IP over Tailscale or ZeroTier for
/// playing with friends.
#[derive(Debug)]
pub struct UdpTransport {
    socket: UdpSocket,
    buffer: Box<[u8]>,
}

impl UdpTransport {
    /// Binds to `addr`, e.g. `"0.0.0.0:7777"` to host or `"0.0.0.0:0"` for any free port.
    pub fn bind(addr: impl ToSocketAddrs) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            // One byte spare, so an oversized datagram shows up as too long instead of fitting.
            buffer: vec![0; MAX_DATAGRAM + 1].into_boxed_slice(),
        })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

impl Transport for UdpTransport {
    type Addr = SocketAddr;

    fn send(&mut self, to: SocketAddr, datagram: &[u8]) {
        // A failed send is just a lost datagram; the layer above resends what matters.
        let _ = self.socket.send_to(datagram, to);
    }

    fn receive(&mut self, into: &mut Vec<(SocketAddr, Vec<u8>)>) {
        loop {
            match self.socket.recv_from(&mut self.buffer) {
                Ok((len, from)) if len <= MAX_DATAGRAM => {
                    into.push((from, self.buffer[..len].to_vec()))
                }
                Ok(_) => {}
                // Windows reports an earlier send to a closed port as a reset on the next receive.
                Err(err) if err.kind() == ErrorKind::ConnectionReset => {}
                Err(_) => return,
            }
        }
    }
}
