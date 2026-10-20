//! What one datagram carries. Encoded with `bincode`, as `ggrs` encodes its own messages.

use bincode::Options;
use serde::{Deserialize, Serialize};

/// No datagram we send is bigger; decoding refuses anything claiming more, so a corrupt length
/// prefix can't make us allocate.
pub(crate) const MAX_DATAGRAM: usize = 8 * 1024;

/// Opens every [`Frame::Hello`], so stray traffic on the port is never taken for a peer.
pub(crate) const MAGIC: u32 = u32::from_be_bytes(*b"NET1");

#[derive(Serialize, Deserialize)]
pub(crate) enum Frame {
    /// Asks to connect; repeated until answered.
    Hello {
        magic: u32,
        protocol: u32,
        game: String,
    },
    Welcome,
    Refuse(Refusal),
    Ggrs(ggrs::Message),
    /// One fragment of a reliable message; `last` marks the fragment that completes it.
    Data {
        seq: u32,
        last: bool,
        bytes: Vec<u8>,
    },
    /// Every fragment below `next` has arrived, and so has `next + 1 + i` for each set bit `i`.
    Ack {
        next: u32,
        beyond: u32,
    },
    /// The sender is closing the connection.
    Bye,
}

/// Why a peer turned down a connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    /// The refusing peer runs a different build; these are its versions.
    Version { protocol: u32, game: String },
    /// The peer isn't hosting.
    NotListening,
}

impl Frame {
    pub(crate) fn encode(&self) -> Vec<u8> {
        options().serialize(self).expect("frames always encode")
    }

    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        options().deserialize(bytes).ok()
    }
}

fn options() -> impl Options {
    bincode::DefaultOptions::new().with_limit(MAX_DATAGRAM as u64)
}
