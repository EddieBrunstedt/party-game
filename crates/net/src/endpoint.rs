use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::frame::{Frame, MAGIC, Refusal};
use crate::reliable::Channel;
use crate::{Clock, PROTOCOL_VERSION, Transport};

/// How often an unanswered hello is repeated.
const HELLO_INTERVAL: Duration = Duration::from_millis(250);
/// Goodbyes aren't acked, so each is sent this many times to survive ordinary loss.
const BYE_COPIES: usize = 3;

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    /// The game's build version; peers must match it exactly, as well as [`PROTOCOL_VERSION`].
    pub game_version: String,
    /// Accept hellos from peers we didn't [`Endpoint::connect`] to (hosting, and the mesh of
    /// minigame peers, where everyone connects to everyone).
    pub listen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetEvent<A> {
    /// The handshake with this peer completed, whichever side started it.
    Connected(A),
    /// A peer we connected to turned us down.
    Refused(A, Refusal),
    /// A whole reliable message, delivered once and in the order it was sent.
    Message(A, Vec<u8>),
    /// The peer said goodbye ([`Endpoint::disconnect`]).
    Disconnected(A),
}

/// [`Endpoint::send`] to a peer that never connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotConnected;

/// One machine's side of the network: handshakes, a reliable ordered channel per peer, and the
/// `ggrs` socket, all sharing one [`Transport`]. Clones are handles to the same endpoint, so the
/// board protocol and a minigame's `ggrs` session can use it at the same time.
pub struct Endpoint<T: Transport>(Arc<Mutex<Inner<T>>>);

impl<T: Transport> Clone for Endpoint<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

struct Inner<T: Transport> {
    transport: T,
    clock: Clock,
    config: EndpointConfig,
    peers: BTreeMap<T::Addr, Peer>,
    events: Vec<NetEvent<T::Addr>>,
    ggrs_inbox: Vec<(T::Addr, ggrs::Message)>,
}

struct Peer {
    state: PeerState,
    channel: Channel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerState {
    /// Waiting for the answer to a hello, last sent at this time (`None`: not sent yet).
    Connecting {
        hello_at: Option<Duration>,
    },
    Connected,
    Refused,
}

impl<T: Transport> Endpoint<T> {
    pub fn new(transport: T, clock: Clock, config: EndpointConfig) -> Self {
        Self(Arc::new(Mutex::new(Inner {
            transport,
            clock,
            config,
            peers: BTreeMap::new(),
            events: Vec::new(),
            ggrs_inbox: Vec::new(),
        })))
    }

    /// Starts the handshake with `addr`; [`NetEvent::Connected`] or [`NetEvent::Refused`]
    /// follows from [`Endpoint::poll`]. Messages sent meanwhile go out once connected.
    pub fn connect(&self, addr: T::Addr) {
        let mut inner = self.lock();
        if inner.peers.contains_key(&addr) {
            return;
        }
        inner.peers.insert(
            addr,
            Peer {
                state: PeerState::Connecting { hello_at: None },
                channel: Channel::default(),
            },
        );
        inner.tick();
    }

    /// Sends a reliable message: it arrives once, whole, and after everything sent before it.
    pub fn send(&self, to: T::Addr, message: &[u8]) -> Result<(), NotConnected> {
        let mut inner = self.lock();
        let peer = inner.peers.get_mut(&to).ok_or(NotConnected)?;
        if peer.state == PeerState::Refused {
            return Err(NotConnected);
        }
        peer.channel.push(message);
        inner.tick();
        Ok(())
    }

    /// Receives, repeats hellos, resends whatever is overdue, and returns what happened since
    /// the last poll. Call it every frame.
    pub fn poll(&self) -> Vec<NetEvent<T::Addr>> {
        let mut inner = self.lock();
        inner.pump();
        inner.tick();
        std::mem::take(&mut inner.events)
    }

    /// Says goodbye to `addr` and forgets it; it gets [`NetEvent::Disconnected`]. Anything
    /// still unacked to it is dropped, so send a last message a moment before, not right before.
    pub fn disconnect(&self, addr: T::Addr) {
        let mut inner = self.lock();
        if inner.peers.remove(&addr).is_some() {
            let bye = Frame::Bye.encode();
            for _ in 0..BYE_COPIES {
                inner.transport.send(addr, &bye);
            }
        }
    }

    pub fn is_connected(&self, addr: T::Addr) -> bool {
        self.lock()
            .peers
            .get(&addr)
            .is_some_and(|peer| peer.state == PeerState::Connected)
    }

    /// Every peer whose handshake has completed, in address order.
    pub fn connected_peers(&self) -> Vec<T::Addr> {
        let inner = self.lock();
        let connected = inner
            .peers
            .iter()
            .filter(|(_, peer)| peer.state == PeerState::Connected);
        connected.map(|(addr, _)| *addr).collect()
    }

    /// The socket for a `ggrs` session. It only talks to connected peers.
    pub fn ggrs_socket(&self) -> GgrsSocket<T> {
        GgrsSocket(self.clone())
    }

    /// Runs `f` on the transport, e.g. to read a UDP socket's local address.
    pub fn with_transport<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.lock().transport)
    }

    fn lock(&self) -> MutexGuard<'_, Inner<T>> {
        self.0
            .lock()
            .expect("a thread panicked while holding the endpoint")
    }
}

impl<T: Transport> Inner<T> {
    fn send_frame(&mut self, to: T::Addr, frame: &Frame) {
        self.transport.send(to, &frame.encode());
    }

    fn hello(&self) -> Frame {
        Frame::Hello {
            magic: MAGIC,
            protocol: PROTOCOL_VERSION,
            game: self.config.game_version.clone(),
        }
    }

    /// Reads every waiting datagram and acts on it.
    fn pump(&mut self) {
        let mut datagrams = Vec::new();
        self.transport.receive(&mut datagrams);
        for (from, bytes) in datagrams {
            if let Some(frame) = Frame::decode(&bytes) {
                self.handle(from, frame);
            }
        }
    }

    fn handle(&mut self, from: T::Addr, frame: Frame) {
        let state = self.peers.get(&from).map(|peer| peer.state);
        match frame {
            Frame::Hello {
                magic,
                protocol,
                game,
            } => {
                if magic != MAGIC {
                    return;
                }
                if protocol != PROTOCOL_VERSION || game != self.config.game_version {
                    let refusal = Refusal::Version {
                        protocol: PROTOCOL_VERSION,
                        game: self.config.game_version.clone(),
                    };
                    self.send_frame(from, &Frame::Refuse(refusal));
                    return;
                }
                match state {
                    None if !self.config.listen => {
                        self.send_frame(from, &Frame::Refuse(Refusal::NotListening));
                        return;
                    }
                    None | Some(PeerState::Connecting { .. }) => self.mark_connected(from),
                    // It refused us earlier, which is final.
                    Some(PeerState::Refused) => return,
                    // Our welcome was lost: welcome it again.
                    Some(PeerState::Connected) => {}
                }
                self.send_frame(from, &Frame::Welcome);
            }
            Frame::Welcome => {
                if matches!(state, Some(PeerState::Connecting { .. })) {
                    self.mark_connected(from);
                }
            }
            Frame::Refuse(refusal) => {
                if let Some(PeerState::Connecting { .. }) = state {
                    self.peers.get_mut(&from).expect("state came from it").state =
                        PeerState::Refused;
                    self.events.push(NetEvent::Refused(from, refusal));
                }
            }
            Frame::Ggrs(message) => {
                if state == Some(PeerState::Connected) {
                    self.ggrs_inbox.push((from, message));
                }
            }
            Frame::Data { seq, last, bytes } => {
                // The peer only sends data once it has welcomed us, so data stands in for a
                // welcome that got lost.
                match state {
                    Some(PeerState::Connecting { .. }) => self.mark_connected(from),
                    Some(PeerState::Connected) => {}
                    None | Some(PeerState::Refused) => return,
                }
                let peer = self.peers.get_mut(&from).expect("connected above");
                let (ack, messages) = peer.channel.receive(seq, last, bytes);
                self.send_frame(from, &ack);
                let delivered = messages
                    .into_iter()
                    .map(|message| NetEvent::Message(from, message));
                self.events.extend(delivered);
            }
            Frame::Bye => {
                if self.peers.remove(&from).is_some() {
                    self.events.push(NetEvent::Disconnected(from));
                }
            }
            Frame::Ack { next, beyond } => {
                if state == Some(PeerState::Connected) {
                    let now = self.clock.now();
                    let peer = self.peers.get_mut(&from).expect("state came from it");
                    peer.channel.acknowledge(now, next, beyond);
                }
            }
        }
    }

    fn mark_connected(&mut self, addr: T::Addr) {
        let peer = self.peers.entry(addr).or_insert_with(|| Peer {
            state: PeerState::Connected,
            channel: Channel::default(),
        });
        peer.state = PeerState::Connected;
        self.events.push(NetEvent::Connected(addr));
    }

    /// Sends due hellos and every reliable fragment that is new or overdue.
    fn tick(&mut self) {
        let now = self.clock.now();
        let hello = self.hello().encode();
        let mut outgoing = Vec::new();
        for (&addr, peer) in &mut self.peers {
            match &mut peer.state {
                PeerState::Connecting { hello_at } => {
                    if hello_at.is_none_or(|at| now >= at + HELLO_INTERVAL) {
                        *hello_at = Some(now);
                        outgoing.push((addr, hello.clone()));
                    }
                }
                PeerState::Connected => {
                    let frames = peer.channel.transmit(now);
                    outgoing.extend(frames.iter().map(|frame| (addr, frame.encode())));
                }
                PeerState::Refused => {}
            }
        }
        for (to, datagram) in outgoing {
            self.transport.send(to, &datagram);
        }
    }
}

/// `ggrs`' view of an [`Endpoint`]: its messages travel as unreliable frames on the same
/// transport, to and from connected peers only.
pub struct GgrsSocket<T: Transport>(Endpoint<T>);

impl<T: Transport> ggrs::NonBlockingSocket<T::Addr> for GgrsSocket<T> {
    fn send_to(&mut self, msg: &ggrs::Message, addr: &T::Addr) {
        let mut inner = self.0.lock();
        if inner
            .peers
            .get(addr)
            .is_some_and(|peer| peer.state == PeerState::Connected)
        {
            inner.send_frame(*addr, &Frame::Ggrs(msg.clone()));
        }
    }

    fn receive_all_messages(&mut self) -> Vec<(T::Addr, ggrs::Message)> {
        let mut inner = self.0.lock();
        // Anything else that arrives waits in the endpoint for its next poll.
        inner.pump();
        std::mem::take(&mut inner.ggrs_inbox)
    }
}
