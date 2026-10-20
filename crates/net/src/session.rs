//! An online session from the lobby on: the [`Host`] keeps the real [`LobbyState`] and tells
//! every [`Client`] about it; clients send their controllers' actions. Starting hands everyone
//! the seed and every machine's address, for the minigames' full mesh. Then the board phase:
//! clients send intents for their seats, and the host applies them and sends [`BoardUpdate`]s.

use std::collections::BTreeMap;

use bincode::Options;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sim_board::{Event, Game, Intent, Rejected};
use sim_core::{Format, MAX_PLAYERS, Outcome, PlayerSetup, Seat, checksum};

use crate::board::BoardUpdate;
use crate::lobby::{LobbyAction, LobbyState, PeerId};
use crate::{Endpoint, NetEvent, Refusal, Transport};

/// Reliable messages can be big (a whole board snapshot) but never this big.
const MAX_MESSAGE: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "A: Serialize + DeserializeOwned")]
enum HostMessage<A> {
    Welcome {
        you: PeerId,
    },
    Lobby(LobbyState),
    Start(Start<A>),
    Refused(JoinRefusal),
    Board(BoardUpdate),
    /// A player read the instruction card (relayed from whichever machine they're on).
    Ready(Seat),
    /// Free play: everyone plays this minigame now.
    Launch(Launch),
    /// Free play: back to the host choosing a minigame.
    BackToPicker,
}

/// A minigame for every machine to start, chosen by the host in free play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub id: String,
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum ClientMessage {
    Act(LobbyAction),
    /// A decision for a seat this machine plays.
    Intent {
        seat: Seat,
        intent: Intent,
    },
    /// This machine's copy of the board went wrong; send the whole game.
    NeedSnapshot,
    /// One of this machine's players read the instruction card.
    Ready(Seat),
}

/// What everyone needs to begin, sent by the host when it starts the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "A: Serialize + DeserializeOwned")]
pub struct Start<A> {
    pub seed: u64,
    pub lobby: LobbyState,
    /// Every client machine and where the host reaches it, in id order. (Clients reach the
    /// host where they connected to it.)
    pub clients: Vec<(PeerId, A)>,
}

/// Why joining a session failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinRefusal {
    /// The host runs another build.
    Version { protocol: u32, game: String },
    /// Nobody is hosting at that address (or it's another player's machine).
    NotHosting,
    /// Every seat that could be taken by another machine is taken.
    Full,
    /// The host already started.
    Started,
}

fn encode<T: Serialize>(message: &T) -> Vec<u8> {
    options()
        .serialize(message)
        .expect("messages always encode")
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    options().deserialize(bytes).ok()
}

fn options() -> impl Options {
    bincode::DefaultOptions::new().with_limit(MAX_MESSAGE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostEvent {
    /// A machine connected and was given this id.
    Joined(PeerId),
    /// A machine left; its seats are free again.
    Left(PeerId),
    /// The lobby changed because of a client.
    LobbyChanged,
    /// A client decided for one of its own seats. Apply it with [`Host::apply`].
    Intent { seat: Seat, intent: Intent },
    /// A client's board went wrong: answer with [`Host::send_snapshot`].
    NeedSnapshot(PeerId),
    /// A client's player read the instruction card (already passed on to the other clients).
    Ready(Seat),
}

pub struct Host<T: Transport> {
    endpoint: Endpoint<T>,
    lobby: LobbyState,
    clients: BTreeMap<T::Addr, PeerId>,
    started: bool,
}

impl<T: Transport> Host<T>
where
    T::Addr: Serialize + DeserializeOwned,
{
    /// Hosts on `endpoint`, which must be listening.
    pub fn new(endpoint: Endpoint<T>, lobby: LobbyState) -> Self {
        Self {
            endpoint,
            lobby,
            clients: BTreeMap::new(),
            started: false,
        }
    }

    pub fn lobby(&self) -> &LobbyState {
        &self.lobby
    }

    pub fn endpoint(&self) -> &Endpoint<T> {
        &self.endpoint
    }

    /// Connected machines other than this one, in id order.
    pub fn clients(&self) -> Vec<(PeerId, T::Addr)> {
        let mut clients: Vec<_> = self.clients.iter().map(|(addr, id)| (*id, *addr)).collect();
        clients.sort();
        clients
    }

    /// An action from one of the host machine's own controllers.
    pub fn act(&mut self, action: LobbyAction) -> bool {
        let changed = self.lobby.apply(PeerId::HOST, action);
        if changed {
            self.broadcast(&HostMessage::Lobby(self.lobby.clone()));
        }
        changed
    }

    pub fn poll(&mut self) -> Vec<HostEvent> {
        let mut events = Vec::new();
        for event in self.endpoint.poll() {
            match event {
                NetEvent::Connected(addr) => {
                    if let Some(id) = self.admit(addr) {
                        events.push(HostEvent::Joined(id));
                    }
                }
                NetEvent::Message(addr, bytes) => {
                    let Some(&peer) = self.clients.get(&addr) else {
                        continue;
                    };
                    match decode(&bytes) {
                        Some(ClientMessage::Act(action)) => {
                            if !self.started && self.lobby.apply(peer, action) {
                                self.broadcast(&HostMessage::Lobby(self.lobby.clone()));
                                events.push(HostEvent::LobbyChanged);
                            }
                        }
                        // Only for seats that machine plays: the rest aren't its to decide.
                        Some(ClientMessage::Intent { seat, intent }) => {
                            let owner = self.lobby.seats[seat.index()].map(|claim| claim.peer);
                            if self.started && owner == Some(peer) {
                                events.push(HostEvent::Intent { seat, intent });
                            }
                        }
                        Some(ClientMessage::NeedSnapshot) => {
                            events.push(HostEvent::NeedSnapshot(peer))
                        }
                        Some(ClientMessage::Ready(seat)) => {
                            let owner = self.lobby.seats[seat.index()].map(|claim| claim.peer);
                            if owner == Some(peer) {
                                self.broadcast(&HostMessage::Ready(seat));
                                events.push(HostEvent::Ready(seat));
                            }
                        }
                        None => {}
                    }
                }
                NetEvent::Disconnected(addr) => {
                    if let Some(peer) = self.clients.remove(&addr) {
                        if self.lobby.remove_peer(peer) {
                            self.broadcast(&HostMessage::Lobby(self.lobby.clone()));
                        }
                        events.push(HostEvent::Left(peer));
                    }
                }
                NetEvent::Refused(..) => {}
            }
        }
        events
    }

    /// Gives a newly connected machine the lowest free id and the lobby, or turns it away.
    fn admit(&mut self, addr: T::Addr) -> Option<PeerId> {
        if self.clients.contains_key(&addr) {
            return None;
        }
        let taken = |id: u8| self.clients.values().any(|p| p.0 == id);
        let free = (1..MAX_PLAYERS as u8).find(|&id| !taken(id));
        let refusal = match free {
            _ if self.started => Some(JoinRefusal::Started),
            None => Some(JoinRefusal::Full),
            Some(_) => None,
        };
        if let Some(refusal) = refusal {
            // The client says goodbye once it reads this.
            let _ = self
                .endpoint
                .send(addr, &encode(&HostMessage::<T::Addr>::Refused(refusal)));
            return None;
        }
        let id = PeerId(free.expect("checked above"));
        self.clients.insert(addr, id);
        let _ = self
            .endpoint
            .send(addr, &encode(&HostMessage::<T::Addr>::Welcome { you: id }));
        let _ = self.endpoint.send(
            addr,
            &encode(&HostMessage::<T::Addr>::Lobby(self.lobby.clone())),
        );
        Some(id)
    }

    /// Starts the session for everyone. No one can join after this.
    pub fn start(&mut self, seed: u64) -> Start<T::Addr> {
        self.started = true;
        let start = Start {
            seed,
            lobby: self.lobby.clone(),
            clients: self.clients(),
        };
        self.broadcast(&HostMessage::Start(start.clone()));
        start
    }

    /// Sends every client the whole game, to start the board phase.
    pub fn share_game(&self, game: &Game) {
        self.broadcast(&HostMessage::Board(BoardUpdate::Snapshot(Box::new(
            game.clone(),
        ))));
    }

    /// Sends one client the whole game.
    pub fn send_snapshot(&self, peer: PeerId, game: &Game) {
        if let Some((addr, _)) = self.clients.iter().find(|(_, id)| **id == peer) {
            let message =
                HostMessage::<T::Addr>::Board(BoardUpdate::Snapshot(Box::new(game.clone())));
            let _ = self.endpoint.send(*addr, &encode(&message));
        }
    }

    /// Applies a decision to the real game and tells every client. Clients' intents come from
    /// [`HostEvent::Intent`]; the host's own players' and bots' come straight here.
    pub fn apply(
        &self,
        game: &mut Game,
        seat: Seat,
        intent: Intent,
    ) -> Result<Vec<Event>, Rejected> {
        let events = game.apply(seat, intent)?;
        let checksum = checksum(game);
        self.broadcast(&HostMessage::Board(BoardUpdate::Applied {
            seat,
            intent,
            checksum,
        }));
        Ok(events)
    }

    /// Applies a minigame's outcome to the real game and tells every client.
    pub fn apply_outcome(&self, game: &mut Game, outcome: Outcome) -> Result<Vec<Event>, Rejected> {
        let events = game.apply_minigame_outcome(outcome)?;
        let checksum = checksum(game);
        self.broadcast(&HostMessage::Board(BoardUpdate::Outcome {
            outcome,
            checksum,
        }));
        Ok(events)
    }

    /// One of the host's players read the instruction card.
    pub fn ready(&self, seat: Seat) {
        self.broadcast(&HostMessage::Ready(seat));
    }

    /// Free play: every client starts this minigame.
    pub fn launch(&self, launch: Launch) {
        self.broadcast(&HostMessage::Launch(launch));
    }

    /// Free play: every client goes back to waiting while the host picks.
    pub fn back_to_picker(&self) {
        self.broadcast(&HostMessage::BackToPicker);
    }

    /// Says goodbye to every client.
    pub fn close(self) {
        for addr in self.clients.keys() {
            self.endpoint.disconnect(*addr);
        }
    }

    fn broadcast(&self, message: &HostMessage<T::Addr>) {
        let bytes = encode(message);
        for addr in self.clients.keys() {
            let _ = self.endpoint.send(*addr, &bytes);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientEvent<A> {
    /// In the lobby, as this machine.
    Welcomed(PeerId),
    /// The lobby changed; read it with [`Client::lobby`].
    LobbyChanged,
    Started(Start<A>),
    Refused(JoinRefusal),
    /// The host closed the session or quit.
    HostLeft,
    /// News from the board: apply it with [`crate::board::follow`] (or take the snapshot).
    Board(BoardUpdate),
    /// A player (on any machine) read the instruction card.
    Ready(Seat),
    Launch(Launch),
    BackToPicker,
}

pub struct Client<T: Transport> {
    endpoint: Endpoint<T>,
    host: T::Addr,
    me: Option<PeerId>,
    lobby: Option<LobbyState>,
    /// The other clients, once started.
    others: Vec<(PeerId, T::Addr)>,
}

impl<T: Transport> Client<T>
where
    T::Addr: Serialize + DeserializeOwned,
{
    pub fn connect(endpoint: Endpoint<T>, host: T::Addr) -> Self {
        endpoint.connect(host);
        Self {
            endpoint,
            host,
            me: None,
            lobby: None,
            others: Vec::new(),
        }
    }

    /// Every other machine in the session and where to reach it (the host first), once started.
    pub fn peers(&self) -> Vec<(PeerId, T::Addr)> {
        let mut peers = vec![(PeerId::HOST, self.host)];
        peers.extend(self.others.iter().copied());
        peers
    }

    pub fn host(&self) -> T::Addr {
        self.host
    }

    pub fn endpoint(&self) -> &Endpoint<T> {
        &self.endpoint
    }

    /// This machine's id, once the host has welcomed it.
    pub fn me(&self) -> Option<PeerId> {
        self.me
    }

    /// The host's lobby as last heard.
    pub fn lobby(&self) -> Option<&LobbyState> {
        self.lobby.as_ref()
    }

    /// Sends an action from one of this machine's controllers; the result arrives as a new lobby.
    pub fn act(&self, action: LobbyAction) {
        let _ = self
            .endpoint
            .send(self.host, &encode(&ClientMessage::Act(action)));
    }

    /// Decides for one of this machine's seats. Nothing changes until the host's update comes back.
    pub fn send_intent(&self, seat: Seat, intent: Intent) {
        let message = ClientMessage::Intent { seat, intent };
        let _ = self.endpoint.send(self.host, &encode(&message));
    }

    /// One of this machine's players read the instruction card.
    pub fn ready(&self, seat: Seat) {
        let _ = self
            .endpoint
            .send(self.host, &encode(&ClientMessage::Ready(seat)));
    }

    /// Asks for the whole game, when this machine's copy no longer matches the host's.
    pub fn request_snapshot(&self) {
        let _ = self
            .endpoint
            .send(self.host, &encode(&ClientMessage::NeedSnapshot));
    }

    pub fn poll(&mut self) -> Vec<ClientEvent<T::Addr>> {
        let mut events = Vec::new();
        for event in self.endpoint.poll() {
            match event {
                NetEvent::Message(from, bytes) if from == self.host => {
                    match decode::<HostMessage<T::Addr>>(&bytes) {
                        Some(HostMessage::Welcome { you }) => {
                            self.me = Some(you);
                            events.push(ClientEvent::Welcomed(you));
                        }
                        Some(HostMessage::Lobby(lobby)) => {
                            self.lobby = Some(lobby);
                            events.push(ClientEvent::LobbyChanged);
                        }
                        Some(HostMessage::Start(start)) => {
                            // Minigames need every machine to reach every other: connect to
                            // the other clients too.
                            let me = self.me;
                            self.others = start
                                .clients
                                .iter()
                                .copied()
                                .filter(|(id, _)| Some(*id) != me)
                                .collect();
                            for (_, addr) in &self.others {
                                self.endpoint.connect(*addr);
                            }
                            events.push(ClientEvent::Started(start));
                        }
                        Some(HostMessage::Refused(refusal)) => {
                            self.endpoint.disconnect(self.host);
                            events.push(ClientEvent::Refused(refusal));
                        }
                        Some(HostMessage::Board(update)) => events.push(ClientEvent::Board(update)),
                        Some(HostMessage::Ready(seat)) => events.push(ClientEvent::Ready(seat)),
                        Some(HostMessage::Launch(launch)) => {
                            events.push(ClientEvent::Launch(launch))
                        }
                        Some(HostMessage::BackToPicker) => events.push(ClientEvent::BackToPicker),
                        None => {}
                    }
                }
                NetEvent::Refused(from, refusal) if from == self.host => {
                    events.push(ClientEvent::Refused(match refusal {
                        Refusal::Version { protocol, game } => {
                            JoinRefusal::Version { protocol, game }
                        }
                        Refusal::NotListening => JoinRefusal::NotHosting,
                    }));
                }
                NetEvent::Disconnected(from) if from == self.host => {
                    events.push(ClientEvent::HostLeft)
                }
                _ => {}
            }
        }
        events
    }

    /// Says goodbye to the host, which frees this machine's seats.
    pub fn leave(self) {
        self.endpoint.disconnect(self.host);
    }
}
