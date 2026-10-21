//! Networking (ADR 0004): a datagram [`Transport`] with UDP and in-memory backends, and an
//! [`Endpoint`] on top that does the version handshake, a reliable ordered channel for board
//! messages, and the `ggrs` socket for minigames, all over the same transport.
//! [`rollback::Rollback`] drives any minigame through `ggrs` on top of it, and [`check`] is the
//! headless online quality bar. [`lobby`] holds the join screen's rules, [`session`] the host
//! and client that share them, and [`board`] how clients follow the host's board.
//!
//! This is not simulation code, so it may use the wall clock, but only through [`Clock`], which
//! tests replace with a manual one so that delays and resends are reproducible.

pub mod board;
pub mod check;
mod clock;
mod endpoint;
mod faults;
mod frame;
pub mod lobby;
mod memory;
mod reliable;
pub mod rollback;
pub mod session;
mod udp;

use std::fmt::Debug;
use std::hash::Hash;

pub use clock::Clock;
pub use endpoint::{Endpoint, EndpointConfig, GgrsSocket, NetEvent, NotConnected, PEER_TIMEOUT};
pub use faults::{Faults, Faulty};
pub use frame::Refusal;
pub use memory::{MemoryAddr, MemoryNetwork, MemoryTransport};
pub use udp::UdpTransport;

/// Bumped by hand whenever the wire format changes (frames here, or board protocol messages).
/// Peers with a different protocol or game version refuse each other at the handshake.
pub const PROTOCOL_VERSION: u32 = 6;

/// Moves whole datagrams between addresses, with no promises: they may be lost, duplicated,
/// delayed or reordered. Everything reliable is built on top, in [`Endpoint`].
pub trait Transport: Send + 'static {
    type Addr: Copy + Ord + Hash + Debug + Send + Sync + 'static;

    fn send(&mut self, to: Self::Addr, datagram: &[u8]);

    /// Appends every datagram that has arrived since the last call.
    fn receive(&mut self, into: &mut Vec<(Self::Addr, Vec<u8>)>);
}

/// So the game can pick a transport at runtime (plain UDP, or UDP with `--net-delay`).
impl<A> Transport for Box<dyn Transport<Addr = A>>
where
    A: Copy + Ord + Hash + Debug + Send + Sync + 'static,
{
    type Addr = A;

    fn send(&mut self, to: A, datagram: &[u8]) {
        (**self).send(to, datagram);
    }

    fn receive(&mut self, into: &mut Vec<(A, Vec<u8>)>) {
        (**self).receive(into);
    }
}
