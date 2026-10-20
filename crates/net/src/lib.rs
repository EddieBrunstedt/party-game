//! Networking (ADR 0004): a datagram [`Transport`] with UDP and in-memory backends, and an
//! [`Endpoint`] on top that does the version handshake, a reliable ordered channel for board
//! messages, and the `ggrs` socket for minigames, all over the same transport.
//!
//! This is not simulation code, so it may use the wall clock, but only through [`Clock`], which
//! tests replace with a manual one so that delays and resends are reproducible.

mod clock;
mod endpoint;
mod faults;
mod frame;
mod memory;
mod reliable;
mod udp;

use std::fmt::Debug;
use std::hash::Hash;

pub use clock::Clock;
pub use endpoint::{Endpoint, EndpointConfig, GgrsSocket, NetEvent, NotConnected};
pub use faults::{Faults, Faulty};
pub use frame::Refusal;
pub use memory::{MemoryAddr, MemoryNetwork, MemoryTransport};
pub use udp::UdpTransport;

/// Bumped by hand whenever the wire format changes (frames here, or board protocol messages).
/// Peers with a different protocol or game version refuse each other at the handshake.
pub const PROTOCOL_VERSION: u32 = 1;

/// Moves whole datagrams between addresses, with no promises: they may be lost, duplicated,
/// delayed or reordered. Everything reliable is built on top, in [`Endpoint`].
pub trait Transport: Send + 'static {
    type Addr: Copy + Ord + Hash + Debug + Send + Sync + 'static;

    fn send(&mut self, to: Self::Addr, datagram: &[u8]);

    /// Appends every datagram that has arrived since the last call.
    fn receive(&mut self, into: &mut Vec<(Self::Addr, Vec<u8>)>);
}
