//! The game's side of online play: hosting or joining a session over UDP, and which addresses
//! friends can reach this machine at. The rules and messages live in the `net` crate.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};

use bevy::prelude::*;
use net::board::BoardUpdate;
use net::lobby::{LobbyAction, LobbyState, PeerId};
use net::session::{Client, ClientEvent, Host, HostEvent, JoinRefusal, Start};
use net::{Clock, Endpoint, EndpointConfig, Transport, UdpTransport};
use sim_board::{Event, Game, Intent, Rejected};
use sim_core::{Outcome, Seat};

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
            .add_systems(PreUpdate, poll.run_if(resource_exists::<Online>))
            .add_systems(Update, leave_when_the_host_does);
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
}

/// Reads the network once a frame, before any screen looks at it.
fn poll(mut online: ResMut<Online>, mut events: MessageWriter<OnlineEvent>) {
    events.write_batch(online.poll());
}

/// Wherever this machine is (lobby, board, results), the host leaving ends the session.
fn leave_when_the_host_does(
    mut events: MessageReader<OnlineEvent>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<crate::flow::AppState>>,
) {
    if events.read().any(|event| *event == OnlineEvent::HostLeft) {
        commands.remove_resource::<Online>();
        commands.remove_resource::<OnlineStart>();
        commands.insert_resource(Notice(tr!("notice-host-left")));
        next_state.set(crate::flow::AppState::Title);
    }
}

impl Online {
    pub fn host(port: u16, lobby: LobbyState) -> std::io::Result<Self> {
        let endpoint = endpoint(UdpTransport::bind(("0.0.0.0", port))?, true);
        Ok(Self::Host(Host::new(endpoint, lobby)))
    }

    pub fn join(host: SocketAddr) -> std::io::Result<Self> {
        let endpoint = endpoint(UdpTransport::bind(("0.0.0.0", 0))?, false);
        Ok(Self::Client(Client::connect(endpoint, host)))
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

fn endpoint(udp: UdpTransport, listen: bool) -> Endpoint<NetTransport> {
    let config = EndpointConfig {
        game_version: game_version(),
        listen,
    };
    Endpoint::new(Box::new(udp), Clock::real(), config)
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

/// How this machine hosts, from the command line.
#[derive(Resource, Debug, Clone, Copy)]
pub struct NetSettings {
    pub port: u16,
}

impl Default for NetSettings {
    fn default() -> Self {
        Self { port: DEFAULT_PORT }
    }
}

/// A message for the title screen, e.g. why an online session ended.
#[derive(Resource, Debug, Clone)]
pub struct Notice(pub String);

/// The host started the online session; a client waits here for the board's snapshot.
#[derive(Resource, Debug, Clone)]
pub struct OnlineStart(
    #[expect(
        dead_code,
        reason = "online minigames (next roadmap item) build their mesh from it"
    )]
    pub Start<SocketAddr>,
);

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
