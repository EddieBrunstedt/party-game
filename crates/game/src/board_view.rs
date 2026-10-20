//! The board: plays back `sim_board` events one at a time with little animations, asks humans
//! for their decisions (bots decide by themselves), and hands off to minigames and back.
//!
//! The simulation resolves whole turns at once; the view keeps its own [`Shown`] copy of what's
//! on screen (pawn spaces, coins, stars) and catches up event by event, so nothing is spoiled
//! before its animation plays.

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use net::board::{BoardUpdate, follow};
use sim_board::{Awaiting, Board, Decision, Event, Game, Intent, SpaceId, SpaceKind, bot_intent};
use sim_core::{Button, Format, MAX_PLAYERS, Outcome, Seat};

use crate::audio::{Sfx, Sound};
use crate::characters::{Pose, Roster};
use crate::flow::{AppState, MinigameLaunch, MinigameResult, Origin};
use crate::i18n::{bot_tag, names};
use crate::online::{Online, OnlineEvent};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{Controller, RunSetup, SeatInputs, Seats};
use crate::scenery::{
    emissive_material, night_camera, noise, spawn_fireflies, spawn_lantern_folk,
    spawn_paper_lantern,
};
use crate::tr;

pub struct BoardViewPlugin;

impl Plugin for BoardViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Board),
            (collect_minigame_result, spawn_scene, spawn_hud).chain(),
        )
        .add_systems(Update, sync_online_board.run_if(resource_exists::<Online>))
        .add_systems(
            Update,
            (
                play_events,
                react_to_events,
                decide,
                (move_pawns, animate_star, follow_camera).chain(),
                fork_arrows,
                update_labels,
                update_hud,
            )
                .chain()
                .run_if(in_state(AppState::Board)),
        );
    }
}

/// Bots wait this long before acting, so a human can follow what they do.
const BOT_THINK_SECS: f32 = 0.7;
const PAWN_SCALE: f32 = 0.55;
const SPACE_RADIUS: f32 = 0.55;
const BLUE: LinearRgba = LinearRgba::rgb(0.15, 0.45, 1.0);
const RED: LinearRgba = LinearRgba::rgb(1.0, 0.16, 0.1);
const STAR_GOLD: LinearRgba = LinearRgba::rgb(1.0, 0.8, 0.25);

/// What the screen currently shows. Lags the simulation while events play.
#[derive(Debug, Clone)]
struct Shown {
    space: [SpaceId; MAX_PLAYERS],
    coins: [u32; MAX_PLAYERS],
    stars: [u32; MAX_PLAYERS],
    star: SpaceId,
    round: u32,
    active: Option<Seat>,
    /// The last die result shown, cleared at the end of the turn.
    die: Option<(Seat, u8)>,
    steps_left: Option<u8>,
    /// Turn-order rolls shown so far.
    order_rolls: [Option<u8>; MAX_PLAYERS],
    final_standings: Option<Outcome>,
}

struct Playing {
    event: Event,
    /// Counts events played, so one-shot effects know which they've already reacted to.
    serial: u64,
    started: f32,
    duration: f32,
}

/// A board game in progress. Outlives the board screen: it waits here while minigames play.
#[derive(Resource)]
pub struct BoardSession {
    pub game: Game,
    queue: VecDeque<Event>,
    playing: Option<Playing>,
    played: u64,
    shown: Shown,
    /// When the board last became ready for a decision.
    ready_since: Option<f32>,
    /// The highlighted option at a fork.
    path_choice: usize,
    /// Online client: a decision went to the host and its update hasn't come back yet.
    sent: bool,
    /// The seed of the last minigame launched from here, so it isn't launched twice.
    launched: Option<u64>,
}

impl BoardSession {
    /// Shows `game` as it stands: a new game, or a snapshot from the host mid-game.
    pub fn new(game: Game) -> Self {
        let start = game.board().start();
        let players = game.players();
        let of = |seat: Seat| players.is_active(seat).then(|| *game.player(seat));
        let shown = Shown {
            space: Seat::ALL.map(|s| of(s).map_or(start, |p| p.space)),
            coins: Seat::ALL.map(|s| of(s).map_or(0, |p| p.coins)),
            stars: Seat::ALL.map(|s| of(s).map_or(0, |p| p.stars)),
            star: game.star(),
            round: game.round(),
            active: None,
            die: None,
            steps_left: None,
            order_rolls: [None; MAX_PLAYERS],
            final_standings: None,
        };
        Self {
            game,
            queue: VecDeque::new(),
            playing: None,
            played: 0,
            shown,
            ready_since: None,
            path_choice: 0,
            sent: false,
            launched: None,
        }
    }

    fn idle(&self) -> bool {
        self.playing.is_none() && self.queue.is_empty()
    }

    fn push(&mut self, events: Vec<Event>) {
        self.queue.extend(events);
        self.ready_since = None;
    }
}

/// How long each event holds the screen.
fn duration(event: &Event) -> f32 {
    match event {
        Event::TurnOrderRolled { .. } => 0.9,
        Event::TurnOrderDecided { .. } => 2.4,
        Event::RoundStarted { .. } => 1.6,
        Event::TurnStarted { .. } => 0.9,
        Event::Rolled { .. } => 1.0,
        Event::Moved { .. } => 0.3,
        Event::ReachedFork { .. } | Event::StarOffered { .. } => 0.1,
        Event::StarBought { .. } => 2.2,
        Event::StarDeclined { .. } => 0.7,
        Event::StarCannotAfford { .. } => 1.6,
        Event::StarMoved { .. } => 1.6,
        Event::Landed { .. } => 0.3,
        Event::CoinsChanged { .. } => 0.9,
        Event::TurnEnded { .. } => 0.3,
        Event::MinigameChosen { .. } => 3.2,
        Event::MinigameFinished { .. } => 1.8,
        Event::RoundEnded { .. } => 0.4,
        Event::GameOver { .. } => 0.0,
    }
}

/// Update what's shown as an event starts.
fn show(shown: &mut Shown, event: &Event) {
    match event {
        Event::TurnOrderRolled { seat, value } => shown.order_rolls[seat.index()] = Some(*value),
        Event::RoundStarted { round } => {
            shown.round = *round;
            shown.order_rolls = [None; MAX_PLAYERS];
        }
        Event::TurnStarted { seat } => {
            shown.active = Some(*seat);
            shown.die = None;
            shown.steps_left = None;
        }
        Event::Rolled { seat, value } => {
            shown.die = Some((*seat, *value));
            shown.steps_left = Some(*value);
        }
        Event::Moved { steps_left, .. } => shown.steps_left = Some(*steps_left),
        Event::StarBought { seat, stars, .. } => shown.stars[seat.index()] = *stars,
        Event::StarMoved { to, .. } => shown.star = *to,
        Event::CoinsChanged { seat, total, .. } => shown.coins[seat.index()] = *total,
        Event::TurnEnded { .. } => {
            shown.active = None;
            shown.die = None;
            shown.steps_left = None;
        }
        Event::GameOver { placements } => shown.final_standings = Some(*placements),
        _ => {}
    }
}

/// Update what's shown as an event finishes.
fn finish(shown: &mut Shown, event: &Event) {
    if let Event::Moved { seat, to, .. } = event {
        shown.space[seat.index()] = *to;
    }
}

fn play_events(time: Res<Time>, mut session: ResMut<BoardSession>) {
    let now = time.elapsed_secs();
    let session = &mut *session;
    loop {
        if let Some(playing) = &session.playing {
            if now - playing.started < playing.duration {
                return;
            }
            finish(&mut session.shown, &playing.event);
            session.playing = None;
        }
        let Some(event) = session.queue.pop_front() else {
            return;
        };
        if let Event::GameOver { .. } = event {
            // Online, every machine logs this; they must agree.
            let sum = sim_core::checksum(&session.game);
            info!("board game over, checksum {sum:016x}");
        }
        show(&mut session.shown, &event);
        session.played += 1;
        session.playing = Some(Playing {
            duration: duration(&event),
            event,
            serial: session.played,
            started: now,
        });
    }
}

/// A sound (and sometimes sparks over the pawn) as each event starts playing.
fn react_to_events(
    mut commands: Commands,
    session: Res<BoardSession>,
    pawns: Query<(&Pawn, &Transform)>,
    effects: Res<Effects>,
    sfx: Sfx,
    mut seen: Local<u64>,
) {
    let Some(playing) = &session.playing else {
        return;
    };
    if *seen == playing.serial {
        return;
    }
    *seen = playing.serial;
    let above = |seat: Seat| {
        pawns
            .iter()
            .find(|(p, _)| p.seat == seat)
            .map(|(_, t)| t.translation + Vec3::Y * 1.2)
    };
    let mut spark = |seat: Seat, color: Vec4| {
        if let Some(at) = above(seat) {
            particles::burst(
                &mut commands,
                &effects,
                DespawnOnExit(AppState::Board),
                at,
                color,
            );
        }
    };
    match &playing.event {
        Event::TurnOrderRolled { .. } | Event::Rolled { .. } => sfx.play(Sound::DiceStop),
        Event::RoundStarted { .. } => sfx.play(Sound::Ready),
        Event::TurnStarted { .. } => sfx.play(Sound::TurnStart),
        Event::Moved { .. } => sfx.play_varied(Sound::Step, playing.serial as u32),
        Event::StarBought { seat, .. } => {
            sfx.play(Sound::Star);
            spark(*seat, hdr(STAR_GOLD, 8.0));
        }
        Event::StarDeclined { .. } => sfx.play(Sound::Back),
        Event::StarCannotAfford { .. } => sfx.play(Sound::TooSlow),
        Event::StarMoved { .. } => sfx.play(Sound::Fall),
        Event::CoinsChanged { seat, delta, .. } if *delta > 0 => {
            sfx.play(Sound::CoinsUp);
            spark(*seat, hdr(STAR_GOLD, 4.0));
        }
        Event::CoinsChanged { seat, delta, .. } if *delta < 0 => {
            sfx.play(Sound::CoinsDown);
            spark(*seat, hdr(RED, 3.0));
        }
        Event::MinigameChosen { .. } => sfx.play(Sound::Join),
        Event::MinigameFinished { .. } => sfx.play(Sound::RoundWin),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn decide(
    mut commands: Commands,
    time: Res<Time>,
    seats: Res<Seats>,
    inputs: Res<SeatInputs>,
    mut session: ResMut<BoardSession>,
    online: Option<Res<Online>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if !session.idle() || session.sent {
        return;
    }
    let now = time.elapsed_secs();
    let ready_since = *session.ready_since.get_or_insert(now);
    let intent = match session.game.awaiting() {
        Awaiting::Intent { seat, decision } => match seats.0[seat.index()] {
            Some(Controller::Bot(difficulty)) => (now - ready_since >= BOT_THINK_SECS)
                .then(|| bot_intent(&session.game, seat, difficulty))
                .flatten()
                .map(|intent| (seat, intent)),
            // Another machine decides; its decision arrives through the host.
            Some(Controller::Remote { .. }) => None,
            _ => human_intent(&mut session, &inputs, seat, &decision).map(|intent| (seat, intent)),
        },
        // Back from a minigame online, a client waits here for the host's outcome.
        Awaiting::Minigame(plan) if session.launched == Some(plan.seed) => None,
        Awaiting::Minigame(plan) => {
            session.launched = Some(plan.seed);
            commands.insert_resource(MinigameLaunch {
                id: plan.id,
                setup: RunSetup {
                    seed: plan.seed,
                    players: plan.players,
                    format: plan.format,
                    config: default(),
                },
                origin: Origin::Board,
            });
            next_state.set(AppState::Minigame);
            None
        }
        Awaiting::GameOver => {
            let anyone_pressed = Seat::ALL
                .into_iter()
                .any(|seat| inputs.just_pressed(seat, Button::A));
            let no_humans = Seat::ALL.into_iter().all(|seat| !seats.is_human(seat));
            if now - ready_since > 2.0 && (anyone_pressed || (no_humans && now - ready_since > 8.0))
            {
                next_state.set(AppState::Results);
            }
            None
        }
    };
    if let Some((seat, intent)) = intent {
        let result = match online.as_deref() {
            Some(online) => online.decide(&mut session.game, seat, intent),
            None => session.game.apply(seat, intent),
        };
        match result {
            Ok(events) => {
                session.path_choice = 0;
                session.sent = matches!(online.as_deref(), Some(Online::Client(_)));
                session.push(events);
            }
            Err(rejected) => warn!("board refused {intent:?} from {seat:?}: {rejected}"),
        }
    }
}

/// Online: a client follows the host's board (the snapshot that starts it, then every
/// decision and outcome), and the host applies clients' decisions and resends the game to a
/// client whose copy went wrong.
fn sync_online_board(
    mut events: MessageReader<OnlineEvent>,
    online: Res<Online>,
    mut session: Option<ResMut<BoardSession>>,
    state: Res<State<AppState>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    // A snapshot replaces the session, and updates in the same batch go on top of it.
    let mut fresh: Option<BoardSession> = None;
    for event in events.read() {
        match (event, online.as_ref()) {
            (OnlineEvent::Board(BoardUpdate::Snapshot(game)), Online::Client(_)) => {
                fresh = Some(BoardSession::new((**game).clone()));
            }
            (OnlineEvent::Board(update), Online::Client(client)) => {
                let Some(target) = fresh.as_mut().or(session.as_deref_mut()) else {
                    warn!("a board update before the snapshot");
                    continue;
                };
                match follow(&mut target.game, update) {
                    Ok(events) => {
                        target.sent = false;
                        target.push(events);
                    }
                    Err(desync) => {
                        error!(
                            "out of step with the host's board ({desync}); asking for the whole game"
                        );
                        client.request_snapshot();
                    }
                }
            }
            (OnlineEvent::Intent { seat, intent }, Online::Host(host)) => {
                let Some(session) = session.as_deref_mut() else {
                    continue;
                };
                match host.apply(&mut session.game, *seat, *intent) {
                    Ok(events) => session.push(events),
                    // Sent twice before the first came back, say: nothing to do.
                    Err(rejected) => debug!("refused {intent:?} from {seat:?}: {rejected}"),
                }
            }
            (OnlineEvent::NeedSnapshot(peer), Online::Host(host)) => {
                if let Some(session) = &session {
                    warn!("{peer:?} went out of step; sending it the whole game");
                    host.send_snapshot(*peer, &session.game);
                }
            }
            _ => {}
        }
    }
    if let Some(fresh) = fresh {
        info!("joined the host's board at round {}", fresh.game.round());
        commands.insert_resource(fresh);
        if *state.get() != AppState::Board {
            next_state.set(AppState::Board);
        }
    }
}

fn human_intent(
    session: &mut BoardSession,
    inputs: &SeatInputs,
    seat: Seat,
    decision: &Decision,
) -> Option<Intent> {
    let a = inputs.just_pressed(seat, Button::A);
    match decision {
        Decision::TurnOrderRoll | Decision::Roll => a.then_some(Intent::Roll),
        Decision::BuyStar { .. } => {
            if a {
                Some(Intent::BuyStar(true))
            } else if inputs.just_pressed(seat, Button::B) {
                Some(Intent::BuyStar(false))
            } else {
                None
            }
        }
        Decision::ChoosePath { options } => {
            // Point the stick the way you want to go; the option in that direction lights up.
            if let (true, Some(stick)) = (inputs.stick_flicked(seat), inputs.stick(seat)) {
                let board = session.game.board();
                let here = space_pos(board, session.game.player(seat).space);
                let wanted = Vec3::new(stick.x, 0.0, -stick.y).normalize_or_zero();
                session.path_choice = options
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| {
                        let score = |id: &&SpaceId| {
                            (space_pos(board, **id) - here)
                                .normalize_or_zero()
                                .dot(wanted)
                        };
                        score(a).total_cmp(&score(b))
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
            a.then_some(Intent::ChoosePath(session.path_choice as u8))
        }
    }
}

/// When a board minigame has just finished, feed its outcome to the board.
fn collect_minigame_result(
    mut commands: Commands,
    mut session: ResMut<BoardSession>,
    result: Option<Res<MinigameResult>>,
    online: Option<Res<Online>>,
) {
    let Some(result) = result else {
        return;
    };
    commands.remove_resource::<MinigameResult>();
    let applied = match online.as_deref() {
        None => session.game.apply_minigame_outcome(result.0),
        Some(online @ Online::Host(_)) => online.apply_outcome(&mut session.game, result.0),
        // A client's board gets the outcome from the host, like every other change.
        Some(Online::Client(_)) => return,
    };
    match applied {
        Ok(events) => session.push(events),
        Err(rejected) => warn!("board refused the minigame outcome: {rejected}"),
    }
}

fn space_pos(board: &Board, id: SpaceId) -> Vec3 {
    let space = board.space(id);
    Vec3::new(space.x, 0.0, space.y)
}

#[derive(Component)]
struct Pawn {
    seat: Seat,
    /// Smoothed facing angle.
    yaw: f32,
}

#[derive(Component)]
struct StarMarker;

#[derive(Component)]
struct ForkArrow {
    index: usize,
}

#[derive(Component)]
struct BoardCamera {
    focus: Vec3,
}

#[derive(Component)]
enum Label {
    /// The die or the steps left, over the active pawn.
    OverPawn,
}

/// "+3" / "-3" drifting up from a pawn.
#[derive(Component)]
struct CoinPopup {
    at: Vec3,
    born: f32,
}

#[derive(Component)]
enum HudText {
    Round,
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

#[derive(Component)]
struct SeatPanelBox(Seat);

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    session: Res<BoardSession>,
    roster: Res<Roster>,
) {
    let scope = DespawnOnExit(AppState::Board);
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 45.0,
        ..default()
    });
    let board = session.game.board();
    let (min, max) = bounds(board);
    let centre = (min + max) / 2.0;
    let focus = session
        .shown
        .active
        .map(|s| space_pos(board, session.shown.space[s.index()]))
        .unwrap_or(centre);
    commands.spawn((
        scope.clone(),
        night_camera(camera_at(focus), (20.0, 48.0)),
        BoardCamera { focus },
    ));
    commands.spawn((
        scope.clone(),
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-6.0, 14.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    spawn_ground(&mut commands, &mut meshes, &mut materials, board, min, max);
    spawn_spaces(&mut commands, &mut meshes, &mut materials, board);
    spawn_trees(&mut commands, &mut meshes, &mut materials, board, min, max);
    let corners = [
        (
            Vec3::new(min.x - 1.5, 0.0, min.z - 1.5),
            LinearRgba::rgb(1.0, 0.3, 0.55),
        ),
        (
            Vec3::new(max.x + 1.5, 0.0, min.z - 1.5),
            LinearRgba::rgb(0.25, 0.75, 1.0),
        ),
        (Vec3::new(min.x - 1.5, 0.0, max.z + 1.5), LANTERN_WARM),
        (
            Vec3::new(max.x + 1.5, 0.0, max.z + 1.5),
            LinearRgba::rgb(1.0, 0.3, 0.55),
        ),
        (Vec3::new(centre.x, 0.0, min.z - 2.0), LANTERN_WARM),
        (
            Vec3::new(centre.x, 0.0, max.z + 2.0),
            LinearRgba::rgb(0.25, 0.75, 1.0),
        ),
    ];
    for (pos, color) in corners {
        spawn_paper_lantern(
            &mut commands,
            &mut meshes,
            &mut materials,
            AppState::Board,
            pos,
            color,
        );
    }
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Board,
        60,
        min + Vec3::new(-3.0, 0.4, -3.0),
        max + Vec3::new(3.0, 3.5, 3.0),
    );

    // The Star: two crossed glowing cubes and a light, floating over its space.
    let star_material = materials.add(emissive_material(STAR_GOLD, 6.0));
    let star_cube = meshes.add(Cuboid::new(0.5, 0.5, 0.5));
    commands
        .spawn((
            scope.clone(),
            StarMarker,
            Transform::from_translation(space_pos(board, session.shown.star)),
            Visibility::default(),
        ))
        .with_children(|star| {
            for rotation in [
                Quat::from_rotation_z(PI / 4.0),
                Quat::from_rotation_x(PI / 4.0) * Quat::from_rotation_y(PI / 4.0),
            ] {
                star.spawn((
                    Mesh3d(star_cube.clone()),
                    MeshMaterial3d(star_material.clone()),
                    Transform::from_xyz(0.0, 1.6, 0.0).with_rotation(rotation),
                ));
            }
            star.spawn((
                PointLight {
                    color: Color::from(STAR_GOLD),
                    intensity: 250_000.0,
                    range: 6.0,
                    ..default()
                },
                Transform::from_xyz(0.0, 1.6, 0.0),
            ));
        });

    for seat in session.game.players().seats() {
        let pos = pawn_rest(&session, seat);
        let folk = spawn_lantern_folk(
            &mut commands,
            &mut meshes,
            &mut materials,
            AppState::Board,
            seat,
            roster.of(seat),
            Transform::from_translation(pos).with_scale(Vec3::splat(PAWN_SCALE)),
        );
        commands.entity(folk.root).insert(Pawn { seat, yaw: 0.0 });
    }

    let arrow_mesh = meshes.add(Cone::new(0.22, 0.45));
    let arrow_material = materials.add(emissive_material(LinearRgba::rgb(1.0, 0.95, 0.7), 5.0));
    for index in 0..3 {
        commands.spawn((
            scope.clone(),
            ForkArrow { index },
            Mesh3d(arrow_mesh.clone()),
            MeshMaterial3d(arrow_material.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

fn bounds(board: &Board) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for space in board.spaces() {
        let p = Vec3::new(space.x, 0.0, space.y);
        min = min.min(p);
        max = max.max(p);
    }
    (min, max)
}

fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    board: &Board,
    min: Vec3,
    max: Vec3,
) {
    let tile = meshes.add(Cuboid::new(1.96, 0.6, 1.96));
    let shades: Vec<_> = [(0.05, 0.09, 0.07), (0.04, 0.08, 0.065), (0.06, 0.1, 0.075)]
        .into_iter()
        .map(|(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: 0.95,
                ..default()
            })
        })
        .collect();
    let (lo, hi) = (min - Vec3::splat(8.0), max + Vec3::splat(8.0));
    let mut i = (lo.x / 2.0).floor() as i32;
    while (i as f32) * 2.0 <= hi.x {
        let mut j = (lo.z / 2.0).floor() as i32;
        while (j as f32) * 2.0 <= hi.z {
            let p = Vec3::new(i as f32 * 2.0, 0.0, j as f32 * 2.0);
            // Tiles near the path sit flat; farther out the ground gets lumpier.
            let near = distance_to_paths(board, p) < 1.6;
            let bump = if near { 0.0 } else { noise(i, j, 7) * 0.35 };
            commands.spawn((
                DespawnOnExit(AppState::Board),
                Mesh3d(tile.clone()),
                MeshMaterial3d(shades[(noise(i, j, 8) * 2.999) as usize].clone()),
                Transform::from_xyz(p.x, -0.32 + bump, p.z),
            ));
            j += 1;
        }
        i += 1;
    }
}

fn distance_to_paths(board: &Board, p: Vec3) -> f32 {
    let mut best = f32::MAX;
    for space in board.spaces() {
        let a = Vec3::new(space.x, 0.0, space.y);
        for next in &space.next {
            let b = space_pos(board, *next);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            best = best.min((a + ab * t).distance(p));
        }
    }
    best
}

fn spawn_spaces(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    board: &Board,
) {
    let disc = meshes.add(Cylinder::new(SPACE_RADIUS, 0.14));
    let rim = meshes.add(Cylinder::new(SPACE_RADIUS + 0.08, 0.1));
    let blue = materials.add(emissive_material(BLUE, 1.3));
    let red = materials.add(emissive_material(RED, 1.3));
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.11, 0.1),
        perceptual_roughness: 0.9,
        ..default()
    });
    let step = meshes.add(Cuboid::new(0.16, 0.06, 0.16));
    let step_material = materials.add(emissive_material(LANTERN_WARM, 0.8));
    for space in board.spaces() {
        let p = Vec3::new(space.x, 0.0, space.y);
        let material = match space.kind {
            SpaceKind::Blue => blue.clone(),
            SpaceKind::Red => red.clone(),
        };
        commands.spawn((
            DespawnOnExit(AppState::Board),
            Mesh3d(rim.clone()),
            MeshMaterial3d(stone.clone()),
            Transform::from_translation(p + Vec3::Y * 0.02),
        ));
        commands.spawn((
            DespawnOnExit(AppState::Board),
            Mesh3d(disc.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(p + Vec3::Y * 0.07),
        ));
        // Little glowing stepping stones along each arrow show which way the path runs.
        for next in &space.next {
            let q = space_pos(board, *next);
            let gap = q.distance(p) - 2.0 * SPACE_RADIUS;
            let count = (gap / 0.45).floor().max(1.0) as usize;
            for k in 0..count {
                let t = (SPACE_RADIUS + gap * (k as f32 + 0.5) / count as f32) / q.distance(p);
                commands.spawn((
                    DespawnOnExit(AppState::Board),
                    Mesh3d(step.clone()),
                    MeshMaterial3d(step_material.clone()),
                    Transform::from_translation(p.lerp(q, t) + Vec3::Y * 0.03),
                ));
            }
        }
    }
}

fn spawn_trees(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    board: &Board,
    min: Vec3,
    max: Vec3,
) {
    let trunk = meshes.add(Cuboid::new(0.3, 1.2, 0.3));
    let canopy = meshes.add(Cuboid::new(1.2, 1.0, 1.2));
    let bark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.05, 0.04),
        ..default()
    });
    let leaves = materials.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.09, 0.07),
        perceptual_roughness: 0.9,
        ..default()
    });
    for k in 0..90 {
        let p = Vec3::new(
            min.x - 6.0 + noise(k, 1, 40) * (max.x - min.x + 12.0),
            0.0,
            min.z - 6.0 + noise(k, 1, 41) * (max.z - min.z + 12.0),
        );
        // Keep well clear of the path: the camera looks over pawns from the +z side.
        if distance_to_paths(board, p) < 3.2 {
            continue;
        }
        let size = 0.7 + noise(k, 1, 42) * 0.7;
        commands
            .spawn((
                DespawnOnExit(AppState::Board),
                Transform::from_translation(p)
                    .with_scale(Vec3::splat(size))
                    .with_rotation(Quat::from_rotation_y(noise(k, 1, 43) * TAU)),
                Visibility::default(),
            ))
            .with_children(|tree| {
                tree.spawn((
                    Mesh3d(trunk.clone()),
                    MeshMaterial3d(bark.clone()),
                    Transform::from_xyz(0.0, 0.6, 0.0),
                ));
                tree.spawn((
                    Mesh3d(canopy.clone()),
                    MeshMaterial3d(leaves.clone()),
                    Transform::from_xyz(0.0, 1.6, 0.0),
                ));
                tree.spawn((
                    Mesh3d(canopy.clone()),
                    MeshMaterial3d(leaves.clone()),
                    Transform::from_xyz(0.1, 2.35, -0.05).with_scale(Vec3::splat(0.65)),
                ));
            });
    }
}

/// Where a pawn stands when not walking: on its space, nudged aside if it has company.
fn pawn_rest(session: &BoardSession, seat: Seat) -> Vec3 {
    let shown = &session.shown;
    let space = shown.space[seat.index()];
    let board = session.game.board();
    let crowd = session
        .game
        .players()
        .seats()
        .filter(|s| shown.space[s.index()] == space)
        .count();
    let offset = if crowd > 1 {
        let angle = seat.0 as f32 / MAX_PLAYERS as f32 * TAU + PI / 4.0;
        Vec3::new(angle.cos(), 0.0, angle.sin()) * 0.38
    } else {
        Vec3::ZERO
    };
    space_pos(board, space) + offset + Vec3::Y * 0.14
}

fn walk_progress(playing: &Playing, now: f32) -> f32 {
    ((now - playing.started) / playing.duration.max(1e-3)).clamp(0.0, 1.0)
}

fn move_pawns(
    time: Res<Time>,
    session: Res<BoardSession>,
    mut pawns: Query<(&mut Pawn, &mut Transform, &mut Pose)>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let board = session.game.board();
    let shown = &session.shown;
    let final_winners: Vec<Seat> = shown
        .final_standings
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (mut pawn, mut transform, mut pose) in &mut pawns {
        let seat = pawn.seat;
        let mut target = pawn_rest(&session, seat);
        let mut heading = None;
        match session.playing.as_ref().map(|p| (&p.event, p)) {
            Some((
                Event::Moved {
                    seat: mover,
                    from,
                    to,
                    ..
                },
                playing,
            )) if *mover == seat => {
                let t = walk_progress(playing, now);
                let (a, b) = (space_pos(board, *from), space_pos(board, *to));
                target = a.lerp(b, t) + Vec3::Y * (0.14 + (t * PI).sin() * 0.45);
                heading = Some(b - a);
                transform.translation = target;
            }
            _ => {
                let follow = 1.0 - (-dt * 14.0).exp();
                transform.translation = transform.translation.lerp(target, follow);
            }
        }
        // Celebrate a star, or the win at the end.
        let cheering = matches!(
            session.playing.as_ref().map(|p| &p.event),
            Some(Event::StarBought { seat: s, .. }) if *s == seat
        ) || final_winners.contains(&seat);
        *pose = if cheering {
            Pose::Celebrate
        } else {
            Pose::Idle
        };
        let yaw = match heading {
            Some(dir) => dir.x.atan2(dir.z),
            // Idle pawns turn to face the camera.
            None => 0.0,
        };
        let diff = (yaw - pawn.yaw + PI).rem_euclid(TAU) - PI;
        pawn.yaw += diff * (1.0 - (-dt * 12.0).exp());
        transform.rotation = Quat::from_rotation_y(pawn.yaw);
    }
}

fn animate_star(
    time: Res<Time>,
    session: Res<BoardSession>,
    mut star: Query<&mut Transform, With<StarMarker>>,
) {
    let now = time.elapsed_secs();
    let base = space_pos(session.game.board(), session.shown.star);
    for mut transform in &mut star {
        // A freshly moved star drops in from the sky.
        let drop = match session.playing.as_ref() {
            Some(
                p @ Playing {
                    event: Event::StarMoved { .. },
                    ..
                },
            ) => {
                let t = walk_progress(p, now);
                (1.0 - t) * (1.0 - t) * 6.0
            }
            _ => 0.0,
        };
        transform.translation = base + Vec3::Y * (drop + (now * 1.7).sin() * 0.12);
        transform.rotation = Quat::from_rotation_y(now * 1.1);
    }
}

const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 8.5, 8.0);

fn camera_at(focus: Vec3) -> Transform {
    Transform::from_translation(focus + CAMERA_OFFSET).looking_at(focus, Vec3::Y)
}

/// Follow the active pawn; pull back to the whole board between turns.
fn follow_camera(
    time: Res<Time>,
    session: Res<BoardSession>,
    pawns: Query<(&Pawn, &Transform), Without<BoardCamera>>,
    star: Query<&Transform, (With<StarMarker>, Without<BoardCamera>)>,
    mut camera: Query<(&mut BoardCamera, &mut Transform)>,
) {
    let board = session.game.board();
    let (min, max) = bounds(board);
    // Pull back far enough that the whole board fits, a little nearer the camera than its middle.
    let span = (max.x - min.x).max((max.z - min.z) * 1.6);
    let overview = ((min + max) / 2.0 + Vec3::Z * 1.0, (span / 12.0).max(1.0));
    let event = session.playing.as_ref().map(|p| &p.event);
    let (focus, zoom) = match event {
        Some(Event::StarMoved { .. }) => (
            star.iter()
                .next()
                .map(|t| t.translation)
                .unwrap_or(overview.0),
            1.0,
        ),
        Some(
            Event::MinigameChosen { .. }
            | Event::RoundStarted { .. }
            | Event::TurnOrderDecided { .. }
            | Event::MinigameFinished { .. }
            | Event::GameOver { .. },
        ) => overview,
        _ => match session.shown.active {
            Some(seat) => (
                pawns
                    .iter()
                    .find(|(p, _)| p.seat == seat)
                    .map(|(_, t)| t.translation)
                    .unwrap_or(overview.0),
                1.0,
            ),
            None => overview,
        },
    };
    let follow = 1.0 - (-time.delta_secs() * 3.0).exp();
    for (mut rig, mut transform) in &mut camera {
        rig.focus = rig.focus.lerp(focus, follow);
        let current_zoom = (transform.translation - rig.focus).length() / CAMERA_OFFSET.length();
        let zoom = current_zoom + (zoom - current_zoom) * follow;
        *transform = Transform::from_translation(rig.focus + CAMERA_OFFSET * zoom)
            .looking_at(rig.focus, Vec3::Y);
    }
}

fn fork_arrows(
    time: Res<Time>,
    session: Res<BoardSession>,
    mut arrows: Query<(&ForkArrow, &mut Transform, &mut Visibility)>,
) {
    let options = match (session.idle(), session.game.awaiting()) {
        (
            true,
            Awaiting::Intent {
                decision: Decision::ChoosePath { options },
                seat,
            },
        ) => Some((seat, options)),
        _ => None,
    };
    let now = time.elapsed_secs();
    for (arrow, mut transform, mut visibility) in &mut arrows {
        let Some((seat, options)) = &options else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let Some(option) = options.get(arrow.index) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let board = session.game.board();
        let here = space_pos(board, session.game.player(*seat).space);
        let there = space_pos(board, *option);
        let dir = (there - here).normalize_or_zero();
        let selected = arrow.index == session.path_choice;
        let bob = (now * 6.0).sin() * 0.08;
        *visibility = Visibility::Inherited;
        transform.translation =
            here + dir * (1.1 + if selected { bob } else { 0.0 }) + Vec3::Y * 0.5;
        // Cones point up (+y); tip them over to point along the path.
        transform.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
        transform.scale = Vec3::splat(if selected { 1.4 } else { 0.8 });
    }
}

fn spawn_hud(mut commands: Commands, session: Res<BoardSession>) {
    let scope = DespawnOnExit(AppState::Board);
    commands.spawn((
        scope.clone(),
        HudText::Round,
        Text::new(""),
        TextFont::from_font_size(22.0),
        TextColor(Color::srgb(1.0, 0.93, 0.8)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(30.0),
            right: Val::Px(24.0),
            ..default()
        },
    ));
    commands
        .spawn((
            scope.clone(),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(30.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|top| {
            top.spawn((
                HudText::Banner,
                Text::new(""),
                TextFont::from_font_size(46.0),
                TextColor(Color::srgb(1.0, 0.93, 0.8)),
                TextLayout::justify(Justify::Center),
            ));
            top.spawn((
                HudText::SubBanner,
                Text::new(""),
                TextFont::from_font_size(22.0),
                TextColor(Color::srgba(0.85, 0.88, 1.0, 0.9)),
                TextLayout::justify(Justify::Center),
            ));
        });
    commands
        .spawn((
            scope.clone(),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(20.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for seat in session.game.players().seats() {
                row.spawn((
                    SeatPanelBox(seat),
                    Node {
                        width: Val::Px(250.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(Color::NONE),
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.6)),
                ))
                .with_child((
                    HudText::SeatPanel(seat),
                    Text::new(""),
                    TextFont::from_font_size(18.0),
                    TextColor(Color::WHITE),
                ));
            }
        });
    commands.spawn((
        scope,
        Label::OverPawn,
        Text::new(""),
        TextFont::from_font_size(44.0),
        TextColor(Color::srgb(1.0, 0.95, 0.75)),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        Visibility::Hidden,
    ));
}

/// Over-pawn numbers and coin popups follow their spot in the world.
fn update_labels(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<BoardSession>,
    camera: Query<(&Camera, &GlobalTransform), With<BoardCamera>>,
    pawns: Query<(&Pawn, &Transform)>,
    mut labels: Query<(&Label, &mut Text, &mut Node, &mut Visibility), Without<CoinPopup>>,
    mut popups: Query<(Entity, &CoinPopup, &mut Node, &mut TextColor), Without<Label>>,
    mut seen_coins: Local<u64>,
) {
    let Some((camera, camera_transform)) = camera.iter().next() else {
        return;
    };
    // Event serials restart with each board game.
    if session.is_added() {
        *seen_coins = 0;
    }
    let now = time.elapsed_secs();
    let pawn_pos = |seat: Seat| {
        pawns
            .iter()
            .find(|(p, _)| p.seat == seat)
            .map(|(_, t)| t.translation)
    };
    let screen = |world: Vec3| camera.world_to_viewport(camera_transform, world).ok();
    let shown = &session.shown;

    for (label, mut text, mut node, mut visibility) in &mut labels {
        match label {
            Label::OverPawn => {
                // Over the active pawn: a spinning die while waiting to roll, then the steps left.
                let awaiting_roll = match (session.idle(), session.game.awaiting()) {
                    (
                        true,
                        Awaiting::Intent {
                            seat,
                            decision: Decision::Roll | Decision::TurnOrderRoll,
                        },
                    ) => Some(seat),
                    _ => None,
                };
                let order_roll = match session.playing.as_ref().map(|p| &p.event) {
                    Some(Event::TurnOrderRolled { seat, value }) => Some((*seat, *value)),
                    _ => None,
                };
                let content = if let Some(seat) = awaiting_roll {
                    let spin = (now * 18.0) as u32 % sim_board::DIE_FACES as u32 + 1;
                    Some((seat, spin.to_string()))
                } else if let Some((seat, value)) = order_roll {
                    Some((seat, value.to_string()))
                } else {
                    match (shown.active, shown.steps_left) {
                        (Some(seat), Some(steps)) if steps > 0 => Some((seat, steps.to_string())),
                        _ => None,
                    }
                };
                match content.and_then(|(seat, s)| {
                    let at = screen(pawn_pos(seat)? + Vec3::Y * 2.0)?;
                    Some((at, s))
                }) {
                    Some((at, s)) => {
                        text.0 = s;
                        node.left = Val::Px(at.x - 14.0);
                        node.top = Val::Px(at.y - 30.0);
                        *visibility = Visibility::Inherited;
                    }
                    None => *visibility = Visibility::Hidden,
                }
            }
        }
    }

    // Spawn a popup for each coin change as it starts playing.
    if let Some(Playing {
        event: Event::CoinsChanged { seat, delta, .. },
        serial,
        ..
    }) = session.playing.as_ref()
        && *seen_coins != *serial
    {
        *seen_coins = *serial;
        if let Some(at) = pawn_pos(*seat) {
            let (sign, color) = if *delta >= 0 {
                ("+", Color::srgb(1.0, 0.85, 0.3))
            } else {
                ("", Color::srgb(1.0, 0.35, 0.3))
            };
            commands.spawn((
                DespawnOnExit(AppState::Board),
                CoinPopup {
                    at: at + Vec3::Y * 1.6,
                    born: now,
                },
                Text::new(format!("{sign}{delta}")),
                TextFont::from_font_size(34.0),
                TextColor(color),
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
            ));
        }
    }
    for (entity, popup, mut node, mut color) in &mut popups {
        let age = now - popup.born;
        if age > 1.2 {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(at) = screen(popup.at + Vec3::Y * age * 0.9) {
            node.left = Val::Px(at.x - 16.0);
            node.top = Val::Px(at.y);
        }
        color.0 = color.0.with_alpha((1.0 - age / 1.2).min(1.0));
    }
}

/// A minigame's display name, from its registry id.
pub fn minigame_title(id: &str) -> String {
    tr!(&format!("minigame-{id}"))
}

pub fn format_text(format: &Format, players: impl Iterator<Item = Seat> + Clone) -> String {
    let side = |team: u8| {
        names(
            players
                .clone()
                .filter(move |s| format.team(*s) == Some(team)),
        )
    };
    match format {
        Format::FreeForAll => tr!("format-free-for-all"),
        Format::OneVsThree { .. } | Format::TwoVsTwo { .. } | Format::Duel { .. } => {
            tr!("format-versus", left = side(0), right = side(1))
        }
    }
}

fn banner_text(session: &BoardSession, seats: &Seats) -> (String, String) {
    let shown = &session.shown;
    let players = session.game.players();
    let who = |seat: Seat| format!("{}{}", seat_label(seat), bot_tag(seats.is_bot(seat)));
    let none = String::new;
    if let Some(playing) = &session.playing {
        let banner = match &playing.event {
            Event::TurnOrderRolled { seat, value } => Some((
                tr!("board-turn-order"),
                tr!("board-rolled", player = who(*seat), value = *value as u32),
            )),
            Event::TurnOrderDecided { order } => {
                Some((tr!("board-turn-order"), names(order.iter().copied())))
            }
            Event::RoundStarted { round } => Some((
                tr!("board-round", round = *round),
                tr!("board-round-of", rounds = session.game.rules().rounds),
            )),
            Event::TurnStarted { seat } => Some((tr!("board-turn", player = who(*seat)), none())),
            Event::StarBought { seat, .. } => {
                Some((tr!("board-star-bought", player = seat_label(*seat)), none()))
            }
            Event::StarCannotAfford { price, .. } => Some((
                tr!("board-star-too-poor"),
                tr!("board-star-costs", price = *price),
            )),
            Event::StarMoved { .. } => Some((tr!("board-star-moves"), none())),
            Event::MinigameChosen { id, format, .. } => Some((
                minigame_title(id),
                format_text(format, players.seats().collect::<Vec<_>>().into_iter()),
            )),
            Event::MinigameFinished { outcome, .. } => Some((
                tr!("board-minigame-won", players = names(outcome.winners())),
                tr!("board-prize", coins = session.game.rules().minigame_prize),
            )),
            _ => None,
        };
        if let Some(banner) = banner {
            return banner;
        }
    }
    if let Some(standings) = shown.final_standings {
        let winners: Vec<Seat> = standings.winners().collect();
        return (
            tr!(
                "board-game-won",
                players = names(winners.iter().copied()),
                count = winners.len() as u32
            ),
            tr!("board-see-results"),
        );
    }
    if !session.idle() {
        return (none(), none());
    }
    match session.game.awaiting() {
        Awaiting::Intent { seat, decision } if !seats.is_bot(seat) => {
            let player = seat_label(seat);
            match decision {
                Decision::TurnOrderRoll => (
                    tr!("board-roll-for-order"),
                    tr!("board-hit-die", player = player),
                ),
                Decision::Roll => (none(), tr!("board-press-roll", player = player)),
                Decision::ChoosePath { .. } => (
                    tr!("board-which-way"),
                    tr!("board-point-stick", player = player),
                ),
                Decision::BuyStar { price } => {
                    (tr!("board-buy-star", price = price), tr!("board-yes-no"))
                }
            }
        }
        Awaiting::Intent {
            decision: Decision::BuyStar { price },
            ..
        } => (tr!("board-buy-star", price = price), none()),
        Awaiting::Intent {
            decision: Decision::TurnOrderRoll,
            ..
        } => (tr!("board-roll-for-order"), none()),
        _ => (none(), none()),
    }
}

fn update_hud(
    session: Res<BoardSession>,
    seats: Res<Seats>,
    mut texts: Query<(&HudText, &mut Text)>,
    mut boxes: Query<(&SeatPanelBox, &mut BorderColor)>,
) {
    let (title, subtitle) = banner_text(&session, &seats);
    let shown = &session.shown;
    for (hud, mut text) in &mut texts {
        text.0 = match *hud {
            HudText::Round if shown.round > 0 => tr!(
                "board-round-counter",
                round = shown.round,
                rounds = session.game.rules().rounds
            ),
            HudText::Round => String::new(),
            HudText::Banner => title.clone(),
            HudText::SubBanner => subtitle.clone(),
            HudText::SeatPanel(seat) => {
                let who = bot_tag(seats.is_bot(seat));
                let place = shown
                    .final_standings
                    .and_then(|o| o.placement[seat.index()])
                    .map(|p| format!("   #{p}"))
                    .unwrap_or_default();
                format!(
                    "{}{who}{place}\n{}",
                    seat_label(seat),
                    tr!(
                        "board-panel",
                        stars = shown.stars[seat.index()],
                        coins = shown.coins[seat.index()]
                    )
                )
            }
        };
    }
    for (panel, mut border) in &mut boxes {
        let active = shown.active == Some(panel.0);
        *border = BorderColor::all(if active {
            Color::from(seat_color(panel.0) * 1.5)
        } else {
            Color::NONE
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Input, Inputs};

    fn session() -> BoardSession {
        let game = Game::new(
            Board::meadow(),
            Default::default(),
            4,
            1,
            minigame_registry::ALL,
        );
        BoardSession::new(game)
    }

    /// Seat 0 goes from `previous` to `current` this frame.
    fn inputs(previous: Input, current: Input) -> SeatInputs {
        let mut all = Inputs::default();
        all.previous[0] = previous;
        all.current[0] = current;
        SeatInputs(all)
    }

    fn press(button: Button) -> SeatInputs {
        inputs(Input::default(), Input::default().with(button))
    }

    #[test]
    fn a_rolls_the_die() {
        let mut session = session();
        let decision = Decision::Roll;
        assert_eq!(
            human_intent(&mut session, &press(Button::A), Seat(0), &decision),
            Some(Intent::Roll)
        );
        let held = inputs(
            Input::default().with(Button::A),
            Input::default().with(Button::A),
        );
        assert_eq!(human_intent(&mut session, &held, Seat(0), &decision), None);
    }

    #[test]
    fn a_buys_the_star_and_b_declines() {
        let mut session = session();
        let decision = Decision::BuyStar { price: 20 };
        assert_eq!(
            human_intent(&mut session, &press(Button::A), Seat(0), &decision),
            Some(Intent::BuyStar(true))
        );
        assert_eq!(
            human_intent(&mut session, &press(Button::B), Seat(0), &decision),
            Some(Intent::BuyStar(false))
        );
    }

    #[test]
    fn the_stick_picks_the_path_it_points_along() {
        let mut session = session();
        let board = session.game.board().clone();
        let here = space_pos(&board, session.game.player(Seat(0)).space);
        let options = board.space(board.start()).next.clone();
        // Any two spaces in clearly different directions will do as the fork's options.
        let options = vec![options[0], SpaceId(board.spaces().len() as u16 - 1)];
        let decision = Decision::ChoosePath {
            options: options.clone(),
        };
        for (index, option) in options.iter().enumerate() {
            let dir = (space_pos(&board, *option) - here).normalize();
            let flick = Input {
                stick_x: Input::quantize_axis(dir.x),
                stick_y: Input::quantize_axis(-dir.z),
                ..Input::default()
            };
            let intent = human_intent(
                &mut session,
                &inputs(Input::default(), flick),
                Seat(0),
                &decision,
            );
            assert_eq!(intent, None, "pointing alone doesn't commit");
            assert_eq!(session.path_choice, index);
            let intent = human_intent(
                &mut session,
                &inputs(flick, flick.with(Button::A)),
                Seat(0),
                &decision,
            );
            assert_eq!(intent, Some(Intent::ChoosePath(index as u8)));
        }
    }

    #[test]
    fn minigame_titles_read_nicely() {
        assert_eq!(minigame_title("lantern-push"), "Lantern Push");
        for entry in minigame_registry::ALL {
            assert_ne!(minigame_title(entry.id), format!("minigame-{}", entry.id));
        }
        assert_eq!(minigame_title("quick-draw"), "Quick Draw");
    }

    #[test]
    fn team_formats_name_both_sides() {
        let format = Format::TwoVsTwo {
            team_of: [0, 1, 0, 1],
        };
        assert_eq!(
            format_text(&format, Seat::ALL.into_iter()),
            "P1 & P3  vs  P2 & P4"
        );
    }
}
