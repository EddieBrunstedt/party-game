//! The game's side of online play: hosting or joining a session over UDP, and which addresses
//! friends can reach this machine at. The rules and messages live in the `net` crate.

use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};

use bevy::prelude::*;
use net::board::BoardUpdate;
use net::lobby::{LobbyAction, LobbyState, PeerId};
use net::rollback::SeatOwner;
use net::session::{Client, ClientEvent, Host, HostEvent, JoinRefusal, Launch, Start};
use net::{Clock, Endpoint, EndpointConfig, Faults, Faulty, Transport, UdpTransport};
use sim_board::{Event, Game, Intent, Rejected};
use sim_core::{MAX_PLAYERS, Outcome, Seat};

use crate::flow::{AppState, MinigameLaunch, Origin};
use crate::runner::{RunRules, RunSetup};
use crate::tr;

/// Picked at runtime, so `--net-delay` can wrap UDP in fault injection.
pub type NetTransport = Box<dyn Transport<Addr = SocketAddr>>;

/// The port a host listens on unless told otherwise, and the one a bare address means.
pub const DEFAULT_PORT: u16 = 7777;

/// Peers must run the same build: the handshake compares this.
fn game_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// This machine's part in an online session. Absent on the couch.
#[derive(Resource)]
pub enum Online {
    Host(Host<NetTransport>),
    Client(Client<NetTransport>),
}

pub struct OnlinePlugin;

impl Plugin for OnlinePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OnlineEvent>()
            .init_resource::<NetStats>()
            .init_resource::<RemoteReady>()
            .add_systems(PreUpdate, poll.run_if(resource_exists::<Online>))
            .add_systems(
                Update,
                (leave_when_the_host_does, follow_free_play, note_ready),
            );
    }
}

/// Something the session reported. Every screen reads the ones it cares about.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum OnlineEvent {
    Welcomed,
    Refused(JoinRefusal),
    Started(Start<SocketAddr>),
    HostLeft,
    PeerJoined,
    PeerLeft,
    /// Client: news of the host's board.
    Board(BoardUpdate),
    /// Host: a client decided for one of its seats.
    Intent {
        seat: Seat,
        intent: Intent,
    },
    /// Host: a client's board went out of step and needs the whole game.
    NeedSnapshot(PeerId),
    /// A player on another machine read the instruction card.
    Ready(Seat),
    /// Client, free play: the host started this minigame.
    Launch(Launch),
    /// Client, free play: the host went back to choosing.
    BackToPicker,
}

/// Reads the network once a frame, before any screen looks at it.
fn poll(mut online: ResMut<Online>, mut events: MessageWriter<OnlineEvent>) {
    events.write_batch(online.poll());
}

/// Players on other machines who have read the instruction card. Kept here rather than on
/// the card, because a fast reader's "ready" can arrive before this machine opens its card.
/// The card takes them when it opens and clears them when it closes.
#[derive(Resource, Default)]
pub struct RemoteReady(pub [bool; MAX_PLAYERS]);

fn note_ready(mut events: MessageReader<OnlineEvent>, mut ready: ResMut<RemoteReady>) {
    for event in events.read() {
        if let OnlineEvent::Ready(seat) = event {
            ready.0[seat.index()] = true;
        }
    }
}

/// Free play online, on a client: play what the host launches, wait while it picks.
fn follow_free_play(
    mut events: MessageReader<OnlineEvent>,
    rules: Res<RunRules>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for event in events.read() {
        match event {
            OnlineEvent::Launch(launch) => {
                commands.insert_resource(MinigameLaunch {
                    id: launch.id.clone(),
                    setup: RunSetup {
                        seed: launch.seed,
                        players: launch.players,
                        format: launch.format,
                        config: rules.0,
                    },
                    origin: Origin::FreePlay,
                });
                next_state.set(AppState::Minigame);
            }
            OnlineEvent::BackToPicker => next_state.set(AppState::Pick),
            _ => {}
        }
    }
}

/// How the online minigame is going, for the F3 overlay.
#[derive(Resource, Default)]
pub struct NetStats {
    /// Round trip to each other machine, in milliseconds.
    pings: Vec<(SocketAddr, u128)>,
    /// Ticks replayed by rollbacks, one entry per tick over the last second.
    replayed: VecDeque<u32>,
}

impl NetStats {
    pub fn record(&mut self, replayed: u32, pings: Vec<(SocketAddr, u128)>) {
        self.replayed.push_back(replayed);
        while self.replayed.len() > sim_core::TICK_HZ as usize {
            self.replayed.pop_front();
        }
        self.pings = pings;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// "ping 30/42 ms  rollbacks 12/s (longest 4)", while an online minigame runs.
    pub fn line(&self) -> Option<String> {
        if self.replayed.is_empty() {
            return None;
        }
        let pings: Vec<String> = self.pings.iter().map(|(_, ms)| ms.to_string()).collect();
        let rollbacks = self.replayed.iter().filter(|n| **n > 0).count();
        let longest = self.replayed.iter().max().copied().unwrap_or(0);
        Some(format!(
            "ping {} ms  rollbacks {rollbacks}/s (longest {longest})",
            if pings.is_empty() {
                "-".into()
            } else {
                pings.join("/")
            }
        ))
    }
}

/// Wherever this machine is (lobby, board, results), the host leaving ends the session.
fn leave_when_the_host_does(
    mut events: MessageReader<OnlineEvent>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if events.read().any(|event| *event == OnlineEvent::HostLeft) {
        commands.remove_resource::<Online>();
        commands.remove_resource::<OnlineStart>();
        commands.insert_resource(Notice(tr!("notice-host-left")));
        next_state.set(AppState::Title);
    }
}

impl Online {
    pub fn host(port: u16, lobby: LobbyState, settings: &NetSettings) -> std::io::Result<Self> {
        let endpoint = endpoint(UdpTransport::bind(("0.0.0.0", port))?, true, settings);
        Ok(Self::Host(Host::new(endpoint, lobby)))
    }

    pub fn join(host: SocketAddr, settings: &NetSettings) -> std::io::Result<Self> {
        let endpoint = endpoint(UdpTransport::bind(("0.0.0.0", 0))?, false, settings);
        Ok(Self::Client(Client::connect(endpoint, host)))
    }

    pub fn is_host(&self) -> bool {
        matches!(self, Self::Host(_))
    }

    pub fn endpoint(&self) -> &Endpoint<NetTransport> {
        match self {
            Self::Host(host) => host.endpoint(),
            Self::Client(client) => client.endpoint(),
        }
    }

    /// Who sends each seat's input in a minigame, seen from this machine: its own players,
    /// other machines' players, and the bots and empty seats, which the host sends.
    pub fn seat_owners(&self) -> Option<[SeatOwner<SocketAddr>; MAX_PLAYERS]> {
        let me = self.me()?;
        let lobby = self.lobby()?;
        let peers = match self {
            Self::Host(host) => host.clients(),
            Self::Client(client) => client.peers(),
        };
        let owner = |peer: PeerId| {
            if peer == me {
                Some(SeatOwner::Local)
            } else {
                let addr = peers.iter().find(|(id, _)| *id == peer)?.1;
                Some(SeatOwner::Remote(addr))
            }
        };
        let mut owners = [SeatOwner::Local; MAX_PLAYERS];
        for seat in Seat::ALL {
            let peer = lobby.seats[seat.index()].map_or(PeerId::HOST, |claim| claim.peer);
            owners[seat.index()] = owner(peer)?;
        }
        Some(owners)
    }

    /// One of this machine's players read the instruction card: tell the others.
    pub fn ready(&self, seat: Seat) {
        match self {
            Self::Host(host) => host.ready(seat),
            Self::Client(client) => client.ready(seat),
        }
    }

    /// This machine in the session, once the host has welcomed it.
    pub fn me(&self) -> Option<PeerId> {
        match self {
            Self::Host(_) => Some(PeerId::HOST),
            Self::Client(client) => client.me(),
        }
    }

    pub fn lobby(&self) -> Option<&LobbyState> {
        match self {
            Self::Host(host) => Some(host.lobby()),
            Self::Client(client) => client.lobby(),
        }
    }

    pub fn act(&mut self, action: LobbyAction) {
        match self {
            Self::Host(host) => {
                host.act(action);
            }
            Self::Client(client) => client.act(action),
        }
    }

    pub fn poll(&mut self) -> Vec<OnlineEvent> {
        match self {
            Self::Host(host) => host
                .poll()
                .into_iter()
                .filter_map(|event| match event {
                    HostEvent::Joined(_) => Some(OnlineEvent::PeerJoined),
                    HostEvent::Left(_) => Some(OnlineEvent::PeerLeft),
                    HostEvent::LobbyChanged => None,
                    HostEvent::Intent { seat, intent } => {
                        Some(OnlineEvent::Intent { seat, intent })
                    }
                    HostEvent::NeedSnapshot(peer) => Some(OnlineEvent::NeedSnapshot(peer)),
                    HostEvent::Ready(seat) => Some(OnlineEvent::Ready(seat)),
                })
                .collect(),
            Self::Client(client) => client
                .poll()
                .into_iter()
                .filter_map(|event| match event {
                    ClientEvent::Welcomed(_) => Some(OnlineEvent::Welcomed),
                    ClientEvent::LobbyChanged => None,
                    ClientEvent::Started(start) => Some(OnlineEvent::Started(start)),
                    ClientEvent::Refused(refusal) => Some(OnlineEvent::Refused(refusal)),
                    ClientEvent::HostLeft => Some(OnlineEvent::HostLeft),
                    ClientEvent::Board(update) => Some(OnlineEvent::Board(update)),
                    ClientEvent::Ready(seat) => Some(OnlineEvent::Ready(seat)),
                    ClientEvent::Launch(launch) => Some(OnlineEvent::Launch(launch)),
                    ClientEvent::BackToPicker => Some(OnlineEvent::BackToPicker),
                })
                .collect(),
        }
    }

    /// Applies a decision to the board: the host applies it and tells everyone; a client sends it
    /// to the host and applies nothing until the host's update comes back.
    pub fn decide(
        &self,
        game: &mut Game,
        seat: Seat,
        intent: Intent,
    ) -> Result<Vec<Event>, Rejected> {
        match self {
            Self::Host(host) => host.apply(game, seat, intent),
            Self::Client(client) => {
                client.send_intent(seat, intent);
                Ok(Vec::new())
            }
        }
    }

    /// Host only: a minigame's outcome, applied and shared.
    pub fn apply_outcome(&self, game: &mut Game, outcome: Outcome) -> Result<Vec<Event>, Rejected> {
        match self {
            Self::Host(host) => host.apply_outcome(game, outcome),
            Self::Client(_) => Err(Rejected::NoMinigame),
        }
    }

    /// Leaves the session: a host closes it for everyone, a client gives up its seats.
    pub fn close(self) {
        match self {
            Self::Host(host) => host.close(),
            Self::Client(client) => client.leave(),
        }
    }
}

fn endpoint(udp: UdpTransport, listen: bool, settings: &NetSettings) -> Endpoint<NetTransport> {
    let config = EndpointConfig {
        game_version: game_version(),
        listen,
    };
    let clock = Clock::real();
    let transport: NetTransport = if settings.faults == Faults::default() {
        Box::new(udp)
    } else {
        // `--net-delay` / `--net-loss`: feel a bad connection on one PC.
        let seed = udp.local_addr().map_or(0, |a| a.port() as u64);
        Box::new(Faulty::new(udp, settings.faults, seed, clock.clone()))
    };
    Endpoint::new(transport, clock, config)
}

/// Reads "100.64.1.2", "100.64.1.2:7777" or "pc-name:7777" (names are looked up).
pub fn parse_address(text: &str) -> Result<SocketAddr, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(tr!("connect-no-address"));
    }
    if let Ok(ip) = text.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, DEFAULT_PORT));
    }
    let with_port = if text.contains(':') {
        text.to_string()
    } else {
        format!("{text}:{DEFAULT_PORT}")
    };
    with_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.find(SocketAddr::is_ipv4).or_else(|| addrs.next()))
        .ok_or_else(|| tr!("connect-bad-address", address = text.to_string()))
}

/// Where friends can reach this machine: its LAN address, and its Tailscale one if it has one.
/// Found by asking the OS which local address would route to each network; no packet is sent.
pub fn reachable_addresses() -> Vec<(&'static str, IpAddr)> {
    let route_from = |target: &str| {
        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect(target).ok()?;
        Some(socket.local_addr().ok()?.ip())
    };
    let mut found = Vec::new();
    // Tailscale hands out 100.64.0.0/10 and answers on 100.100.100.100.
    let tailscale = route_from("100.100.100.100:53").filter(|ip| match ip {
        IpAddr::V4(v4) => v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]),
        IpAddr::V6(_) => false,
    });
    if let Some(ip) =
        route_from("192.0.2.1:9").filter(|ip| !ip.is_loopback() && Some(*ip) != tailscale)
    {
        found.push(("lan", ip));
    }
    if let Some(ip) = tailscale {
        found.push(("tailscale", ip));
    }
    found
}

/// Why joining failed, for the screen.
pub fn refusal_text(refusal: &JoinRefusal) -> String {
    match refusal {
        JoinRefusal::Version { game, .. } => {
            tr!(
                "connect-refused-version",
                theirs = game.clone(),
                ours = game_version()
            )
        }
        JoinRefusal::NotHosting => tr!("connect-refused-not-hosting"),
        JoinRefusal::Full => tr!("connect-refused-full"),
        JoinRefusal::Started => tr!("connect-refused-started"),
    }
}

/// How this machine goes online, from the command line.
#[derive(Resource, Debug, Clone, Copy)]
pub struct NetSettings {
    pub port: u16,
    /// What to do to every datagram this machine sends (`--net-delay`, `--net-loss`).
    pub faults: Faults,
}

impl Default for NetSettings {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            faults: Faults::default(),
        }
    }
}

/// A message for the title screen, e.g. why an online session ended.
#[derive(Resource, Debug, Clone)]
pub struct Notice(pub String);

/// The host started the online session; a client waits here for the board's snapshot.
#[derive(Resource, Debug, Clone)]
pub struct OnlineStart;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_address_means_the_default_port() {
        assert_eq!(
            parse_address("100.64.1.2"),
            Ok(SocketAddr::from(([100, 64, 1, 2], DEFAULT_PORT)))
        );
        assert_eq!(
            parse_address(" 127.0.0.1:9000 "),
            Ok(SocketAddr::from(([127, 0, 0, 1], 9000)))
        );
        assert_eq!(
            parse_address("localhost"),
            Ok(SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)))
        );
        assert!(parse_address("").is_err());
        assert!(parse_address("not an address").is_err());
    }
}
