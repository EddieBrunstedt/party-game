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
/// A connected peer we've sent nothing to for this long gets a keepalive, so silence means
/// the peer is gone rather than that there was nothing to say.
const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(200);
/// A connected peer heard nothing from for this long is lost ([`NetEvent::Lost`]).
pub const PEER_TIMEOUT: Duration = Duration::from_secs(5);

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
    /// The peer said goodbye ([`Endpoint::disconnect`]), or told us it had already forgotten us.
    Disconnected(A),
    /// Nothing heard from the peer for [`PEER_TIMEOUT`]: it quit without a word, or the
    /// network between us broke. It's forgotten, as after a goodbye.
    Lost(A),
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
    /// When anything last arrived from it, or when we started connecting.
    heard_at: Duration,
    /// When we last sent it anything.
    sent_at: Duration,
}

impl Peer {
    fn new(state: PeerState, now: Duration) -> Self {
        Self {
            state,
            channel: Channel::default(),
            heard_at: now,
            sent_at: now,
        }
    }
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
        let now = inner.clock.now();
        let peer = Peer::new(PeerState::Connecting { hello_at: None }, now);
        inner.peers.insert(addr, peer);
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

    /// How long since anything arrived from a connected peer.
    pub fn silent_for(&self, addr: T::Addr) -> Option<Duration> {
        let inner = self.lock();
        let peer = inner.peers.get(&addr)?;
        (peer.state == PeerState::Connected).then(|| inner.clock.now() - peer.heard_at)
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

    /// The socket for a new `ggrs` session. It only talks to connected peers, and starts by
    /// dropping whatever `ggrs` traffic was left over from an earlier session.
    pub fn ggrs_socket(&self) -> GgrsSocket<T> {
        self.lock().ggrs_inbox.clear();
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
        let now = self.clock.now();
        if let Some(peer) = self.peers.get_mut(&to) {
            peer.sent_at = now;
        }
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
        let now = self.clock.now();
        if let Some(peer) = self.peers.get_mut(&from) {
            peer.heard_at = now;
        }
        // Traffic for a connection we've forgotten (it timed out here, say, but not there):
        // tell the sender, so it stops waiting on us.
        let for_a_connection = matches!(
            frame,
            Frame::Ggrs(_) | Frame::Data { .. } | Frame::Ack { .. } | Frame::Alive
        );
        if state.is_none() && for_a_connection {
            self.transport.send(from, &Frame::Bye.encode());
            return;
        }
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
                    // A peer that refused us earlier (not listening yet) may connect itself
                    // later: two clients meshing up after a start can cross like that.
                    None | Some(PeerState::Connecting { .. } | PeerState::Refused) => {
                        self.mark_connected(from)
                    }
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
            Frame::Alive => {}
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
        let now = self.clock.now();
        let peer = self
            .peers
            .entry(addr)
            .or_insert_with(|| Peer::new(PeerState::Connected, now));
        peer.state = PeerState::Connected;
        peer.heard_at = now;
        self.events.push(NetEvent::Connected(addr));
    }

    /// Forgets peers that went silent, then sends due hellos, every reliable fragment that is
    /// new or overdue, and keepalives to peers we'd otherwise say nothing to.
    fn tick(&mut self) {
        let now = self.clock.now();
        let silent: Vec<T::Addr> = self
            .peers
            .iter()
            .filter(|(_, peer)| peer.state == PeerState::Connected)
            .filter(|(_, peer)| now - peer.heard_at >= PEER_TIMEOUT)
            .map(|(addr, _)| *addr)
            .collect();
        for addr in silent {
            self.peers.remove(&addr);
            self.events.push(NetEvent::Lost(addr));
        }

        let hello = self.hello().encode();
        let alive = Frame::Alive.encode();
        let mut outgoing = Vec::new();
        for (&addr, peer) in &mut self.peers {
            let before = outgoing.len();
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
                    if outgoing.len() == before && now - peer.sent_at >= KEEPALIVE_INTERVAL {
                        outgoing.push((addr, alive.clone()));
                    }
                }
                PeerState::Refused => {}
            }
            if outgoing.len() > before {
                peer.sent_at = now;
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
