# 0004: Our own transport layer, with UDP first and Steam later

**Status:** Accepted (2026-10-20)

## Context

Online play is friends-only, and mixed couch+online sessions must work. A Steam release is the goal but not certain.
- `matchbox`, the usual WebRTC option for ggrs, has stalled. It targets Bevy 0.18 and ggrs 0.11, so it's incompatible with ggrs 0.13.
- Steam offers lobbies, invites and relayed P2P (SteamNetworkingMessages) through the `steamworks` crate. That crate doesn't depend on Bevy.

## Decision

- **A small transport trait in `net`** carries both ggrs packets (as a `NonBlockingSocket` adapter) and board-phase messages.
- **First backend: plain UDP.** It covers localhost (two game windows on one PC for development) and direct IP. Before Steam, friends play over Tailscale or ZeroTier, which needs no port forwarding and no server.
- **Steam backend** if the game ships on Steam: lobbies and invites, plus SteamNetworkingMessages, behind the same trait.
- **Peers vs players:** a peer is a machine; a player is a seat. One peer can carry 1–3 local players.

## Consequences

- No server infrastructure to run, ever.
- Before Steam, joining by IP is technical, which is acceptable for testing with friends.
- A non-Steam storefront would mean adding another backend later (e.g. WebRTC or a relay).
