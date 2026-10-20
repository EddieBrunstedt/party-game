//! Presentation for Quick Draw. Reads `ActiveRun<QuickDraw>` only; never writes simulation state.

use bevy::prelude::*;
use minigame_quick_draw::{POINTS_TO_WIN, Phase, QuickDraw, Reaction, State};
use sim_core::{Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Character, HOP_SECONDS, Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner, names};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{
    emissive_material, night_camera, noise, spawn_fireflies, spawn_lantern_folk,
    spawn_paper_lantern,
};
use crate::tr;

pub struct QuickDrawViewPlugin;

impl Plugin for QuickDrawViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<QuickDraw>>),
        )
        .add_systems(
            Update,
            (
                animate_lantern,
                animate_characters,
                (fit_camera, shake_camera).chain(),
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<QuickDraw>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<QuickDraw>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
const LANTERN_POS: Vec3 = Vec3::new(0.0, 3.7, -2.6);
const CHARACTER_SPACING: f32 = 2.3;
const FIREFLY_COUNT: usize = 48;

#[derive(Component)]
struct Lantern {
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct LanternLight;

#[derive(Component)]
struct CameraRig {
    base: Transform,
}

#[derive(Component)]
struct Contestant {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
    /// Reaction last seen, to trigger a hop the moment a press registers.
    seen: Reaction,
    seen_generation: u32,
    hop_started: Option<f32>,
}

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<QuickDraw>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 30.0,
        ..default()
    });
    let camera = camera_transform(1.0);
    commands.spawn((
        DespawnOnExit(AppState::Minigame),
        night_camera(camera, (12.0, 34.0)),
        CameraRig { base: camera },
    ));

    commands.spawn((
        DespawnOnExit(AppState::Minigame),
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 350.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-6.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    spawn_ground(&mut commands, &mut meshes, &mut materials);
    spawn_lantern(&mut commands, &mut meshes, &mut materials);
    spawn_side_lanterns(&mut commands, &mut meshes, &mut materials);
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-8.5, 0.5, -6.0),
        Vec3::new(8.5, 3.7, 3.0),
    );

    let players = active.run.players;
    let count = players.count() as f32;
    for (i, seat) in players.seats().enumerate() {
        let x = (i as f32 - (count - 1.0) / 2.0) * CHARACTER_SPACING;
        spawn_character(
            &mut commands,
            &mut meshes,
            &mut materials,
            seat,
            roster.of(seat),
            Vec3::new(x, 0.0, 1.6),
        );
    }
}

fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let tile = meshes.add(Cuboid::new(0.96, 0.4, 0.96));
    let shades: Vec<_> = [
        (0.055, 0.06, 0.075),
        (0.045, 0.05, 0.065),
        (0.07, 0.068, 0.08),
    ]
    .into_iter()
    .map(|(r, g, b)| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 0.92,
            ..default()
        })
    })
    .collect();
    for i in -9..=9 {
        for j in -7..=5 {
            let shade = &shades[(noise(i, j, 1) * 2.999) as usize];
            let y = -0.2 + (noise(i, j, 2) - 0.5) * 0.06;
            commands.spawn((
                DespawnOnExit(AppState::Minigame),
                Mesh3d(tile.clone()),
                MeshMaterial3d(shade.clone()),
                Transform::from_xyz(i as f32, y, j as f32),
            ));
        }
    }
}

fn spawn_lantern(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.09, 0.06, 0.045),
        perceptual_roughness: 0.85,
        ..default()
    });
    let iron = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.055),
        metallic: 0.6,
        perceptual_roughness: 0.4,
        ..default()
    });
    let post_height = LANTERN_POS.y - 0.6;
    commands.spawn((
        DespawnOnExit(AppState::Minigame),
        Mesh3d(meshes.add(Cuboid::new(0.3, post_height, 0.3))),
        MeshMaterial3d(wood.clone()),
        Transform::from_xyz(LANTERN_POS.x, post_height / 2.0, LANTERN_POS.z),
    ));
    let glow = materials.add(emissive_material(LANTERN_WARM, 1.5));
    commands
        .spawn((
            DespawnOnExit(AppState::Minigame),
            Lantern {
                material: glow.clone(),
            },
            Mesh3d(meshes.add(Cuboid::new(0.9, 1.0, 0.9))),
            MeshMaterial3d(glow),
            Transform::from_translation(LANTERN_POS),
        ))
        .with_children(|lantern| {
            lantern.spawn((
                Mesh3d(meshes.add(Cuboid::new(1.15, 0.16, 1.15))),
                MeshMaterial3d(iron.clone()),
                Transform::from_xyz(0.0, 0.58, 0.0),
            ));
            lantern.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.5, 0.2, 0.5))),
                MeshMaterial3d(iron.clone()),
                Transform::from_xyz(0.0, 0.76, 0.0),
            ));
            lantern.spawn((
                Mesh3d(meshes.add(Cuboid::new(1.05, 0.12, 1.05))),
                MeshMaterial3d(iron),
                Transform::from_xyz(0.0, -0.56, 0.0),
            ));
            lantern.spawn((
                LanternLight,
                PointLight {
                    color: Color::from(LANTERN_WARM),
                    intensity: 60_000.0,
                    range: 22.0,
                    radius: 0.4,
                    shadow_maps_enabled: true,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 0.8),
            ));
        });
}

/// Small coloured paper lanterns around the edge: cheap unshadowed lights that give the
/// night scene its colour.
fn spawn_side_lanterns(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let spots = [
        (Vec3::new(-6.5, 0.0, -4.5), LinearRgba::rgb(1.0, 0.3, 0.55)),
        (Vec3::new(6.5, 0.0, -4.5), LinearRgba::rgb(0.25, 0.75, 1.0)),
        (Vec3::new(-7.5, 0.0, 0.5), LANTERN_WARM),
        (Vec3::new(7.5, 0.0, 0.5), LinearRgba::rgb(1.0, 0.3, 0.55)),
        (Vec3::new(-3.6, 0.0, -5.5), LinearRgba::rgb(0.25, 0.75, 1.0)),
        (Vec3::new(3.6, 0.0, -5.5), LANTERN_WARM),
    ];
    for (pos, color) in spots {
        spawn_paper_lantern(commands, meshes, materials, AppState::Minigame, pos, color);
    }
}

fn spawn_character(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    seat: Seat,
    character: Character,
    base: Vec3,
) {
    let folk = spawn_lantern_folk(
        commands,
        meshes,
        materials,
        AppState::Minigame,
        seat,
        character,
        Transform::from_translation(base),
    );
    commands.entity(folk.root).insert(Contestant {
        seat,
        head_material: folk.head_material,
        head_light: folk.head_light,
        seen: Reaction::Waiting,
        seen_generation: 0,
        hop_started: None,
    });
}

/// The unlit lantern smoulders faintly, so the moment it lights is unmistakable.
fn ember(time: f32) -> f32 {
    0.05 + (time * 7.3).sin() * 0.01 + (time * 13.1).sin() * 0.008
}

fn lantern_glow(phase: Phase, time: f32) -> (f32, f32) {
    let tick = 1.0 / TICK_HZ as f32;
    match phase {
        Phase::Drawn { t } => {
            let decay = (-(t as f32) * tick * 3.0).exp();
            (8.0 + 40.0 * decay, 700_000.0 + 2_500_000.0 * decay)
        }
        Phase::RoundOver { t } => {
            let fade = (1.0 - t as f32 * tick * 1.5).max(0.0);
            (ember(time) + 8.0 * fade, 15_000.0 + 700_000.0 * fade)
        }
        Phase::Finished => (6.0, 500_000.0),
        Phase::Intro { .. } | Phase::Waiting { .. } => (ember(time), 15_000.0),
    }
}

fn animate_lantern(
    time: Res<Time>,
    active: Res<ActiveRun<QuickDraw>>,
    lantern: Query<&Lantern>,
    mut light: Query<&mut PointLight, With<LanternLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (glow, lumens) = lantern_glow(active.run.game.phase, time.elapsed_secs());
    for lantern in &lantern {
        if let Some(mut material) = materials.get_mut(&lantern.material) {
            material.emissive = LANTERN_WARM * glow;
        }
    }
    for mut light in &mut light {
        light.intensity = lumens;
    }
}

/// A heartbeat this often while the lantern is dark (not tied to when it lights).
const HEARTBEAT_TICKS: u16 = 45;

/// Sounds and sparks, once per event: the wait, the lantern lighting, every press, the wins.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<QuickDraw>>,
    contestants: Query<(&Contestant, &Transform)>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let head = |seat: Seat| {
        contestants
            .iter()
            .find(|(c, _)| c.seat == seat)
            .map(|(_, t)| t.translation + Vec3::Y * 1.5)
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(&mut commands, &effects, SCOPE, pos, color);
    };

    match (before.phase, now.phase) {
        (_, Phase::Waiting { t, .. }) if t % HEARTBEAT_TICKS == 0 => {
            sfx.play_with(Sound::Heartbeat, 1.0, -3.0);
        }
        (Phase::Waiting { .. }, Phase::Drawn { .. }) => {
            sfx.play(Sound::Ignite);
            spark(LANTERN_POS, hdr(LANTERN_WARM, 8.0));
        }
        (Phase::Drawn { .. }, Phase::RoundOver { .. }) if now.round_winners.iter().any(|w| *w) => {
            sfx.play(Sound::RoundWin);
        }
        (Phase::RoundOver { .. }, Phase::Finished) => sfx.play(Sound::Win),
        _ => {}
    }

    for seat in now.players.seats() {
        let i = seat.index();
        if before.reactions[i] != now.reactions[i] {
            match now.reactions[i] {
                Reaction::Fired { .. } => sfx.play_varied(Sound::Draw, seat.0 as u32),
                Reaction::FalseStart => {
                    sfx.play(Sound::FalseStart);
                    if let Some(at) = head(seat) {
                        spark(at, hdr(LinearRgba::rgb(0.5, 0.5, 0.6), 1.2));
                    }
                }
                Reaction::TooSlow => sfx.play(Sound::TooSlow),
                Reaction::Waiting => {}
            }
        }
        let won_round = !before.round_winners[i] && now.round_winners[i];
        let won_match = now.phase == Phase::Finished
            && before.phase != Phase::Finished
            && now.scores[i] == *now.scores.iter().max().unwrap_or(&0);
        if (won_round || won_match)
            && let Some(at) = head(seat)
        {
            spark(at, hdr(seat_color(seat), 6.0));
            if won_match {
                spark(at + Vec3::Y * 0.6, hdr(LANTERN_WARM, 6.0));
            }
        }
    }
}

fn animate_characters(
    time: Res<Time>,
    active: Res<ActiveRun<QuickDraw>>,
    mut characters: Query<(&mut Contestant, &mut Pose)>,
    mut lights: Query<&mut PointLight, Without<LanternLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let now = time.elapsed_secs();
    let game = &active.run.game;
    for (mut character, mut pose) in &mut characters {
        let seat = character.seat;
        let reaction = game.reactions[seat.index()];
        if character.seen_generation != active.generation {
            character.seen_generation = active.generation;
            character.seen = Reaction::Waiting;
            character.hop_started = None;
        }
        if reaction != character.seen {
            if matches!(reaction, Reaction::Fired { .. }) {
                character.hop_started = Some(now);
            }
            character.seen = reaction;
        }

        let celebrating = match game.phase {
            Phase::RoundOver { .. } => game.round_winners[seat.index()],
            Phase::Finished => game.scores[seat.index()] == *game.scores.iter().max().unwrap_or(&0),
            _ => false,
        };
        let hopping = character
            .hop_started
            .is_some_and(|start| now - start < HOP_SECONDS);
        *pose = match reaction {
            _ if hopping => Pose::Hop,
            _ if celebrating => Pose::Celebrate,
            Reaction::FalseStart | Reaction::TooSlow => Pose::Slump,
            _ if game.phase == Phase::Finished => Pose::Slump,
            _ => Pose::Idle,
        };

        let glow = match reaction {
            Reaction::FalseStart => 0.15,
            Reaction::TooSlow => 0.8,
            _ if celebrating => 5.0,
            Reaction::Fired { .. } => 3.0,
            Reaction::Waiting => 1.8 + (now * 5.0 + seat.0 as f32).sin() * 0.1,
        };
        if let Some(mut material) = materials.get_mut(&character.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(character.head_light) {
            light.intensity = 22_000.0 * glow;
        }
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 1.7, 0.0);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 2.7, 10.0);
/// Half-width of the character line (plus margin) that must stay on screen.
const FRAMED_HALF_WIDTH: f32 = 4.7;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Pull the camera back on narrow windows so all four characters stay in frame.
fn fit_camera(
    windows: Query<&Window, Changed<Window>>,
    mut camera: Query<(&mut CameraRig, &Projection)>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let aspect = window.width() / window.height().max(1.0);
    for (mut rig, projection) in &mut camera {
        let Projection::Perspective(perspective) = projection else {
            continue;
        };
        let visible_half_width = (perspective.fov / 2.0).tan() * aspect * CAMERA_OFFSET.length();
        rig.base = camera_transform((FRAMED_HALF_WIDTH / visible_half_width).max(1.0));
    }
}

fn shake_camera(
    active: Res<ActiveRun<QuickDraw>>,
    time: Res<Time>,
    mut camera: Query<(&CameraRig, &mut Transform)>,
) {
    let strength = match active.run.game.phase {
        Phase::Drawn { t } if t < 12 => 0.07 * (1.0 - t as f32 / 12.0),
        _ => 0.0,
    };
    let t = time.elapsed_secs();
    for (rig, mut transform) in &mut camera {
        *transform = rig.base;
        transform.translation += Vec3::new((t * 91.0).sin(), (t * 73.0).cos(), 0.0) * strength;
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<QuickDraw>>) {
    commands
        .spawn((
            DespawnOnExit(AppState::Minigame),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(36.0),
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
                TextFont::from_font_size(54.0),
                TextColor(Color::srgb(1.0, 0.93, 0.8)),
                TextLayout::justify(Justify::Center),
            ));
            top.spawn((
                HudText::SubBanner,
                Text::new(""),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgba(0.85, 0.88, 1.0, 0.85)),
                TextLayout::justify(Justify::Center),
            ));
        });

    commands
        .spawn((
            DespawnOnExit(AppState::Minigame),
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
            for seat in active.run.players.seats() {
                row.spawn((
                    Node {
                        width: Val::Px(200.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                        ..default()
                    },
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.55)),
                ))
                .with_child((
                    HudText::SeatPanel(seat),
                    Text::new(""),
                    TextFont::from_font_size(18.0),
                    TextColor(Color::WHITE),
                ));
            }
        });
}

fn ms(ticks: u16) -> u32 {
    ticks as u32 * 1000 / TICK_HZ
}

fn banner_text(active: &ActiveRun<QuickDraw>) -> (String, String) {
    let run = &active.run;
    let game: &State = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Waiting { .. } => (
            tr!("quick-draw-wait"),
            tr!("quick-draw-round", round = game.round as u32),
        ),
        Phase::Drawn { .. } => (tr!("quick-draw-draw"), String::new()),
        Phase::RoundOver { .. } => {
            let winners: Vec<Seat> = game
                .players
                .seats()
                .filter(|s| game.round_winners[s.index()])
                .collect();
            match winners.first().map(|s| game.reactions[s.index()]) {
                Some(Reaction::Fired { ticks }) => (
                    tr!(
                        "quick-draw-round-won",
                        count = winners.len() as u32,
                        players = names(winners.into_iter())
                    ),
                    tr!("quick-draw-ms", ms = ms(ticks)),
                ),
                _ => (tr!("quick-draw-nobody"), String::new()),
            }
        }
        Phase::Finished => {
            let outcome = run.outcome().expect("finished games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            (i18n::winners(&winners), active.continue_prompt())
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<QuickDraw>>,
    seats: Res<Seats>,
    mut texts: Query<(&HudText, &mut Text)>,
) {
    let (title, subtitle) = banner_text(&active);
    let game = &active.run.game;
    for (hud, mut text) in &mut texts {
        text.0 = match *hud {
            HudText::Banner => title.clone(),
            HudText::SubBanner => subtitle.clone(),
            HudText::SeatPanel(seat) => {
                let who = bot_tag(seats.is_bot(seat));
                let score = game.scores[seat.index()];
                let pips: String = (0..POINTS_TO_WIN)
                    .map(|i| if i < score { '#' } else { '-' })
                    .collect();
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else {
                    match game.reactions[seat.index()] {
                        Reaction::Fired { ticks } => tr!("quick-draw-ms", ms = ms(ticks)),
                        Reaction::FalseStart => tr!("quick-draw-false-start"),
                        Reaction::TooSlow => tr!("quick-draw-too-slow"),
                        Reaction::Waiting => String::new(),
                    }
                };
                format!("{}{who}   [{pips}]\n{status}", seat_label(seat))
            }
        };
    }
}
