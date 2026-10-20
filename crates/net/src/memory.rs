use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::Transport;

/// An address on a [`MemoryNetwork`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemoryAddr(pub u16);

type Mailboxes = BTreeMap<MemoryAddr, VecDeque<(MemoryAddr, Vec<u8>)>>;

/// Peers in one process, for tests: every datagram arrives instantly and in order (wrap the
/// transports in [`crate::Faulty`] for anything worse). Clones share the same network.
#[derive(Debug, Clone, Default)]
pub struct MemoryNetwork(Arc<Mutex<Mailboxes>>);

impl MemoryNetwork {
    pub fn new() -> Self {
        Self::default()
    }

    /// A transport at `addr`. Datagrams to addresses nobody has bound are dropped.
    pub fn bind(&self, addr: MemoryAddr) -> MemoryTransport {
        self.0.lock().unwrap().entry(addr).or_default();
        MemoryTransport {
            network: self.clone(),
            addr,
        }
    }
}

#[derive(Debug)]
pub struct MemoryTransport {
    network: MemoryNetwork,
    addr: MemoryAddr,
}

impl MemoryTransport {
    pub fn addr(&self) -> MemoryAddr {
        self.addr
    }
}

impl Transport for MemoryTransport {
    type Addr = MemoryAddr;

    fn send(&mut self, to: MemoryAddr, datagram: &[u8]) {
        if let Some(mailbox) = self.network.0.lock().unwrap().get_mut(&to) {
            mailbox.push_back((self.addr, datagram.to_vec()));
        }
    }

    fn receive(&mut self, into: &mut Vec<(MemoryAddr, Vec<u8>)>) {
        let mut mailboxes = self.network.0.lock().unwrap();
        into.extend(
            mailboxes
                .get_mut(&self.addr)
                .into_iter()
                .flat_map(|m| m.drain(..)),
        );
    }
}
