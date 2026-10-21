//! An online session from the lobby on: the [`Host`] keeps the real [`LobbyState`] and tells
//! every [`Client`] about it; clients send their controllers' actions. Starting hands everyone
//! the seed and every machine's address, for the minigames' full mesh. Then the board phase:
//! clients send intents for their seats, and the host applies them and sends [`BoardUpdate`]s.
//!
//! Once started, a machine that drops out keeps its seats: the host lists it as away, and the
//! next machine to connect takes its place (and its seats) back. Each minigame ends on purpose:
//! every machine reports its confirmed end, and the host says when all have it, so nobody stops
//! exchanging inputs while another machine still needs them.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

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
/// A machine joining a started session takes the place of a seated machine this quiet, even
/// before the endpoint gives up on it: a quick restart shouldn't have to wait out the timeout.
const TAKEOVER_SILENCE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "A: Serialize + DeserializeOwned")]
enum HostMessage<A> {
    Welcome {
        you: PeerId,
        /// The session has started, and this machine takes the seats of one that dropped out.
        rejoin: bool,
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
    /// The machines with seats the session is waiting for (gone, or back but not seated yet).
    Away(Vec<PeerId>),
    /// Every other machine and where the host reaches it, after one came back at a new address.
    Peers(Vec<(PeerId, A)>),
    /// Every machine has the end of minigame number `minigame`: stop exchanging its inputs.
    MinigameSettled {
        minigame: u32,
    },
    /// Minigame number `minigame` is abandoned, because a machine dropped out before the host
    /// saw its end. It restarts once everyone is back.
    MinigameStopped {
        minigame: u32,
    },
    /// Play the stopped minigame again from the start, with the same seed.
    MinigameRestart(Launch),
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
    /// Back after dropping out, with a controller in every seat again.
    Back,
    /// This machine has the confirmed end of minigame number `minigame`.
    Finished {
        minigame: u32,
    },
}

/// What everyone needs to begin, sent by the host when it starts the session (and again to a
/// machine that comes back).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "A: Serialize + DeserializeOwned")]
pub struct Start<A> {
    pub seed: u64,
    pub lobby: LobbyState,
    /// Every client machine and where the host reaches it, in id order. (Clients reach the
    /// host where they connected to it.)
    pub clients: Vec<(PeerId, A)>,
    /// How many minigames have ended or been abandoned so far.
    pub minigame: u32,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// A machine connected and was given this id.
    Joined(PeerId),
    /// A machine left before the start; its seats are free again.
    Left(PeerId),
    /// After the start: the machines the session waits for changed (see [`Host::away`]).
    Away(Vec<PeerId>),
    /// A machine that dropped out is seated again: send it the board.
    Back(PeerId),
    /// Every machine still here has the minigame's confirmed end.
    MinigameSettled,
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
    /// The session's seed, once started.
    started: Option<u64>,
    /// Machines with seats that the session waits for: gone, or connected again but not seated.
    away: BTreeSet<PeerId>,
    /// Minigames ended or abandoned so far. It tags each machine's `Finished`, so one sent for
    /// an abandoned attempt isn't counted for the next.
    minigame: u32,
    /// Machines with the current minigame's confirmed end.
    finished: BTreeSet<PeerId>,
    events: Vec<HostEvent>,
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
            started: None,
            away: BTreeSet::new(),
            minigame: 0,
            finished: BTreeSet::new(),
            events: Vec::new(),
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

    /// Machines with seats that the session is waiting for, in id order.
    pub fn away(&self) -> Vec<PeerId> {
        self.away.iter().copied().collect()
    }

    fn present(&self, peer: PeerId) -> bool {
        !self.away.contains(&peer)
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
        for event in self.endpoint.poll() {
            match event {
                NetEvent::Connected(addr) => {
                    if let Some(id) = self.admit(addr) {
                        self.events.push(HostEvent::Joined(id));
                    }
                }
                NetEvent::Message(addr, bytes) => {
                    let Some(&peer) = self.clients.get(&addr) else {
                        continue;
                    };
                    if let Some(message) = decode(&bytes) {
                        self.receive(peer, message);
                    }
                }
                NetEvent::Disconnected(addr) | NetEvent::Lost(addr) => self.lose(addr),
                NetEvent::Refused(..) => {}
            }
        }
        std::mem::take(&mut self.events)
    }

    fn receive(&mut self, peer: PeerId, message: ClientMessage) {
        let owns = |lobby: &LobbyState, seat: Seat| {
            lobby.seats[seat.index()].is_some_and(|claim| claim.peer == peer)
        };
        // A machine taking its seats back says nothing else until it has.
        if !self.present(peer) {
            if message == ClientMessage::Back {
                self.come_back(peer);
            }
            return;
        }
        match message {
            ClientMessage::Act(action) => {
                if self.started.is_none() && self.lobby.apply(peer, action) {
                    self.broadcast(&HostMessage::Lobby(self.lobby.clone()));
                    self.events.push(HostEvent::LobbyChanged);
                }
            }
            // Only for seats that machine plays: the rest aren't its to decide.
            ClientMessage::Intent { seat, intent } => {
                if self.started.is_some() && owns(&self.lobby, seat) {
                    self.events.push(HostEvent::Intent { seat, intent });
                }
            }
            ClientMessage::NeedSnapshot => self.events.push(HostEvent::NeedSnapshot(peer)),
            ClientMessage::Ready(seat) => {
                if owns(&self.lobby, seat) {
                    self.broadcast(&HostMessage::Ready(seat));
                    self.events.push(HostEvent::Ready(seat));
                }
            }
            ClientMessage::Finished { minigame } => {
                if minigame == self.minigame {
                    self.finished.insert(peer);
                    self.settle_if_everyone_finished();
                }
            }
            ClientMessage::Back => {}
        }
    }

    /// A machine said goodbye or went silent. Before the start its seats are free again;
    /// after it, they wait for it to come back.
    fn lose(&mut self, addr: T::Addr) {
        let Some(peer) = self.clients.remove(&addr) else {
            return;
        };
        if self.started.is_none() {
            if self.lobby.remove_peer(peer) {
                self.broadcast(&HostMessage::Lobby(self.lobby.clone()));
            }
            self.events.push(HostEvent::Left(peer));
        } else if self.lobby.peer_joined(peer) && self.away.insert(peer) {
            self.away_changed();
            // It may have been the last one the minigame's end was waiting for.
            self.settle_if_everyone_finished();
        }
    }

    fn away_changed(&mut self) {
        self.broadcast(&HostMessage::Away(self.away()));
        self.events.push(HostEvent::Away(self.away()));
    }

    /// A machine that dropped out has a controller in each of its seats again: it gets the
    /// session's start, and everyone else its new address.
    fn come_back(&mut self, peer: PeerId) {
        self.away.remove(&peer);
        let start = HostMessage::Start(Start {
            seed: self.started.unwrap_or_default(),
            lobby: self.lobby.clone(),
            clients: self.clients(),
            minigame: self.minigame,
        });
        self.send_to(peer, &start);
        self.broadcast_except(peer, &HostMessage::Peers(self.clients()));
        self.away_changed();
        self.events.push(HostEvent::Back(peer));
    }

    /// Gives a newly connected machine the lowest free id and the lobby, or turns it away.
    /// After the start, it can only take the place of a machine that dropped out.
    fn admit(&mut self, addr: T::Addr) -> Option<PeerId> {
        if self.clients.contains_key(&addr) {
            return None;
        }
        if self.started.is_some() {
            return self.readmit(addr);
        }
        let taken = |id: u8| self.clients.values().any(|p| p.0 == id);
        let free = (1..MAX_PLAYERS as u8).find(|&id| !taken(id));
        let refusal = match free {
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
        self.welcome(addr, id, false);
        Some(id)
    }

    fn welcome(&mut self, addr: T::Addr, id: PeerId, rejoin: bool) {
        self.clients.insert(addr, id);
        let welcome = HostMessage::<T::Addr>::Welcome { you: id, rejoin };
        let _ = self.endpoint.send(addr, &encode(&welcome));
        let lobby = HostMessage::<T::Addr>::Lobby(self.lobby.clone());
        let _ = self.endpoint.send(addr, &encode(&lobby));
    }

    /// A machine connecting after the start takes the seats of one that dropped out (the
    /// lowest id first), or of one that has gone quiet; otherwise it's refused.
    fn readmit(&mut self, addr: T::Addr) -> Option<PeerId> {
        let connected = |host: &Self, id: PeerId| host.clients.values().any(|p| *p == id);
        let gone = self.away.iter().copied().find(|id| !connected(self, *id));
        let quiet = || {
            let mut seated = self.clients.iter().filter(|(_, id)| self.present(**id));
            seated
                .find(|(a, _)| {
                    self.endpoint
                        .silent_for(**a)
                        .is_some_and(|silent| silent >= TAKEOVER_SILENCE)
                })
                .map(|(a, id)| (*a, *id))
        };
        let id = match (gone, quiet()) {
            (Some(id), _) => id,
            (None, Some((old, id))) => {
                self.endpoint.disconnect(old);
                self.lose(old);
                id
            }
            (None, None) => {
                // The client says goodbye once it reads this.
                let refused = HostMessage::<T::Addr>::Refused(JoinRefusal::Started);
                let _ = self.endpoint.send(addr, &encode(&refused));
                return None;
            }
        };
        // Its controllers take the seats in order as they press A, numbered afresh.
        self.lobby.renumber_locals(id);
        self.broadcast(&HostMessage::Lobby(self.lobby.clone()));
        self.welcome(addr, id, true);
        Some(id)
    }

    /// Starts the session for everyone. After this, only a machine taking the place of one
    /// that dropped out can join.
    pub fn start(&mut self, seed: u64) -> Start<T::Addr> {
        self.started = Some(seed);
        let start = Start {
            seed,
            lobby: self.lobby.clone(),
            clients: self.clients(),
            minigame: self.minigame,
        };
        self.broadcast(&HostMessage::Start(start.clone()));
        start
    }

    /// The host has the current minigame's confirmed end. Once every machine still here has
    /// it too, [`HostEvent::MinigameSettled`] follows (on every machine).
    pub fn finish_minigame(&mut self) {
        self.finished.insert(PeerId::HOST);
        self.settle_if_everyone_finished();
    }

    fn settle_if_everyone_finished(&mut self) {
        let everyone = self.finished.contains(&PeerId::HOST)
            && self
                .clients
                .values()
                .filter(|id| self.present(**id))
                .all(|id| self.finished.contains(id));
        if everyone {
            self.broadcast(&HostMessage::MinigameSettled {
                minigame: self.minigame,
            });
            self.next_minigame();
            self.events.push(HostEvent::MinigameSettled);
        }
    }

    fn next_minigame(&mut self) {
        self.minigame += 1;
        self.finished.clear();
    }

    /// Abandons the current minigame for everyone, before its end: a machine dropped out.
    pub fn stop_minigame(&mut self) {
        self.broadcast(&HostMessage::MinigameStopped {
            minigame: self.minigame,
        });
        self.next_minigame();
    }

    /// Plays the stopped minigame again from the start, once everyone is back.
    pub fn restart_minigame(&self, launch: Launch) {
        self.broadcast(&HostMessage::MinigameRestart(launch));
    }

    /// The last resort for a machine that won't come back: bots play its seats from now on.
    pub fn hand_to_bots(&mut self, peer: PeerId) {
        if !self.away.remove(&peer) {
            return;
        }
        if let Some((&addr, _)) = self.clients.iter().find(|(_, id)| **id == peer) {
            // Halfway back: too late.
            let refused = HostMessage::<T::Addr>::Refused(JoinRefusal::Started);
            let _ = self.endpoint.send(addr, &encode(&refused));
            self.clients.remove(&addr);
        }
        self.lobby.remove_peer(peer);
        self.broadcast(&HostMessage::Lobby(self.lobby.clone()));
        self.away_changed();
    }

    /// Sends every client the whole game, to start the board phase.
    pub fn share_game(&self, game: &Game) {
        self.broadcast(&HostMessage::Board(BoardUpdate::Snapshot(Box::new(
            game.clone(),
        ))));
    }

    /// Sends one client the whole game.
    pub fn send_snapshot(&self, peer: PeerId, game: &Game) {
        let snapshot = BoardUpdate::Snapshot(Box::new(game.clone()));
        self.send_to(peer, &HostMessage::Board(snapshot));
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

    /// Sends to every machine that is here (not to one still taking its seats back).
    fn broadcast(&self, message: &HostMessage<T::Addr>) {
        self.broadcast_except(PeerId::HOST, message);
    }

    fn broadcast_except(&self, except: PeerId, message: &HostMessage<T::Addr>) {
        let bytes = encode(message);
        for (addr, id) in &self.clients {
            if *id != except && self.present(*id) {
                let _ = self.endpoint.send(*addr, &bytes);
            }
        }
    }

    fn send_to(&self, peer: PeerId, message: &HostMessage<T::Addr>) {
        if let Some((addr, _)) = self.clients.iter().find(|(_, id)| **id == peer) {
            let _ = self.endpoint.send(*addr, &encode(message));
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
    /// The host stopped hearing from this machine and forgot it, or the host went silent.
    HostLost,
    /// The machines with seats the session is waiting for changed.
    Away(Vec<PeerId>),
    /// Every machine has the minigame's confirmed end.
    MinigameSettled,
    /// The minigame is abandoned: a machine dropped out before its end.
    MinigameStopped,
    /// Everyone is back: play the stopped minigame again.
    MinigameRestart(Launch),
}

pub struct Client<T: Transport> {
    endpoint: Endpoint<T>,
    host: T::Addr,
    me: Option<PeerId>,
    lobby: Option<LobbyState>,
    /// The other clients, once started.
    others: Vec<(PeerId, T::Addr)>,
    /// Welcomed into a started session in place of a machine that dropped out, and not
    /// seated yet.
    rejoining: bool,
    /// Minigames ended or abandoned so far, as the host counts them.
    minigame: u32,
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
            rejoining: false,
            minigame: 0,
        }
    }

    /// Welcomed into a started session in place of a machine that dropped out: its controllers
    /// take that machine's seats, then [`Client::back`].
    pub fn rejoining(&self) -> bool {
        self.rejoining
    }

    /// Rejoining: every seat has a controller again. The host answers with the session's start.
    pub fn back(&self) {
        let _ = self.endpoint.send(self.host, &encode(&ClientMessage::Back));
    }

    /// This machine has the minigame's confirmed end. Keep exchanging its inputs until
    /// [`ClientEvent::MinigameSettled`].
    pub fn finish_minigame(&self) {
        let message = ClientMessage::Finished {
            minigame: self.minigame,
        };
        let _ = self.endpoint.send(self.host, &encode(&message));
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
                        Some(HostMessage::Welcome { you, rejoin }) => {
                            self.me = Some(you);
                            self.rejoining = rejoin;
                            events.push(ClientEvent::Welcomed(you));
                        }
                        Some(HostMessage::Lobby(lobby)) => {
                            self.lobby = Some(lobby);
                            events.push(ClientEvent::LobbyChanged);
                        }
                        Some(HostMessage::Start(start)) => {
                            self.rejoining = false;
                            self.minigame = start.minigame;
                            self.lobby = Some(start.lobby.clone());
                            self.mesh(&start.clients);
                            events.push(ClientEvent::Started(start));
                        }
                        Some(HostMessage::Peers(clients)) => self.mesh(&clients),
                        Some(HostMessage::Away(peers)) => events.push(ClientEvent::Away(peers)),
                        Some(HostMessage::MinigameSettled { minigame }) => {
                            self.minigame = minigame + 1;
                            events.push(ClientEvent::MinigameSettled);
                        }
                        Some(HostMessage::MinigameStopped { minigame }) => {
                            self.minigame = minigame + 1;
                            events.push(ClientEvent::MinigameStopped);
                        }
                        Some(HostMessage::MinigameRestart(launch)) => {
                            events.push(ClientEvent::MinigameRestart(launch))
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
                NetEvent::Lost(from) if from == self.host => events.push(ClientEvent::HostLost),
                _ => {}
            }
        }
        events
    }

    /// Minigames need every machine to reach every other: connect to the other clients too,
    /// and forget any that moved (one that dropped out and came back elsewhere).
    fn mesh(&mut self, clients: &[(PeerId, T::Addr)]) {
        let me = self.me;
        let others: Vec<_> = clients
            .iter()
            .copied()
            .filter(|(id, _)| Some(*id) != me)
            .collect();
        for (_, addr) in &self.others {
            if !others.iter().any(|(_, a)| a == addr) {
                self.endpoint.disconnect(*addr);
            }
        }
        for (_, addr) in &others {
            self.endpoint.connect(*addr);
        }
        self.others = others;
    }

    /// Says goodbye to the host, which frees this machine's seats.
    pub fn leave(self) {
        self.endpoint.disconnect(self.host);
    }
}
