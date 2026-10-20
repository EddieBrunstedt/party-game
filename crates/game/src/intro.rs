//! What every minigame shows before it starts: an instruction card with the goal, how to play
//! and the controls, then the 3-2-1 countdown. The card sits outside the simulation, which
//! doesn't tick until every human has pressed A (bots are always ready). The countdown is the
//! first [`COUNTDOWN_TICKS`] of every run, so it's drawn here from [`Run::tick`] alone.
//! A minigame's own code only supplies its strings, by key:
//!
//! - `{id}-title`, `{id}-goal`, `{id}-how` and `{id}-a` (what A does) are required;
//! - `{id}-b` and `{id}-stick` are shown when the minigame uses them;
//! - `{id}-goal-teams` (2v2) and `{id}-goal-solo` (1v3, given `$player`) replace the goal in
//!   those formats when present.

use bevy::prelude::*;
use sim_core::{Button, COUNTDOWN_TICKS, Format, MAX_PLAYERS, Minigame, Run, Seat, TICK_HZ};

use crate::board_view::format_text;
use crate::flow::{AppState, MinigameLaunch};
use crate::i18n::{bot_tag, has_key};
use crate::palette::{NIGHT_SKY, seat_color, seat_label};
use crate::runner::{ActiveRun, MinigameSet, PressLatch, SeatInputs, Seats};
use crate::tr;

/// With nobody to press A, the card stays up this long on its own.
const UNATTENDED_SECS: f32 = 3.0;
/// Once everyone is ready, the card lingers this long so the last "ready" is seen.
const ALL_READY_SECS: f32 = 0.5;

const CREAM: Color = Color::srgb(1.0, 0.93, 0.8);
const DIM: Color = Color::srgba(0.85, 0.88, 1.0, 0.8);
const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);

pub struct IntroPlugin;

impl Plugin for IntroPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            open_card.in_set(MinigameSet::Start),
        )
        .add_systems(OnExit(AppState::Minigame), |mut commands: Commands| {
            commands.remove_resource::<InstructionCard>();
        })
        .add_systems(
            Update,
            update_card.run_if(resource_exists::<InstructionCard>),
        );
    }
}

/// Up while the instruction card is on screen; the minigame's run waits for it to go.
#[derive(Resource)]
pub struct InstructionCard {
    ready: [bool; MAX_PLAYERS],
    opened_at: f32,
    all_ready_at: Option<f32>,
    root: Entity,
}

#[derive(Component)]
struct ReadyChip(Seat);

#[derive(Component)]
pub struct CountdownText;

/// The countdown number to show `tick` ticks into a run, if it's still counting: 3, 2, 1.
pub fn countdown(tick: u32) -> Option<u32> {
    let left = (COUNTDOWN_TICKS as u32)
        .checked_sub(tick)
        .filter(|l| *l > 0)?;
    Some(left.div_ceil(TICK_HZ))
}

/// Draws minigame `M`'s countdown; [`crate::runner::MinigamePlugin`] adds one per minigame.
pub fn show_countdown<M: Minigame>(
    active: Res<ActiveRun<M>>,
    card: Option<Res<InstructionCard>>,
    mut texts: Query<&mut Text, With<CountdownText>>,
) {
    let run: &Run<M> = &active.run;
    // A halted run shows its own banner in the same place.
    let halted = run.paused || run.waiting_for_controller();
    let number = countdown(run.tick).filter(|_| card.is_none() && !halted);
    for mut text in &mut texts {
        text.0 = number.map(|n| n.to_string()).unwrap_or_default();
    }
}

/// The goal line for minigame `id` in `format`.
pub fn goal(id: &str, format: &Format) -> String {
    let solo_key = format!("{id}-goal-solo");
    let teams_key = format!("{id}-goal-teams");
    match format {
        Format::OneVsThree { solo } if has_key(&solo_key) => {
            tr!(&solo_key, player = seat_label(*solo))
        }
        Format::TwoVsTwo { .. } if has_key(&teams_key) => tr!(&teams_key),
        _ => tr!(&format!("{id}-goal")),
    }
}

/// The controls minigame `id` uses, as (control, what it does), in the order shown.
fn controls(id: &str) -> Vec<(String, String)> {
    [
        ("stick", tr!("card-stick")),
        ("a", "A".into()),
        ("b", "B".into()),
    ]
    .into_iter()
    .filter_map(|(suffix, control)| {
        let key = format!("{id}-{suffix}");
        has_key(&key).then(|| (control, tr!(&key)))
    })
    .collect()
}

fn text(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont::from_font_size(size),
        TextColor(color),
        TextLayout::justify(Justify::Center),
    )
}

fn open_card(
    mut commands: Commands,
    launch: Option<Res<MinigameLaunch>>,
    seats: Res<Seats>,
    time: Res<Time>,
) {
    let Some(launch) = launch else {
        return;
    };
    let id = launch.id.as_str();
    let format = launch.setup.format;
    let players = launch.setup.players;
    let ready = std::array::from_fn(|i| !seats.is_human(Seat(i as u8)));

    // Where every minigame's banner goes; views leave theirs empty during the countdown.
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(26.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_child((CountdownText, text(String::new(), 72.0, CREAM)));

    let root = commands
        .spawn((
            SCOPE,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        max_width: Val::Px(820.0),
                        margin: UiRect::horizontal(Val::Px(16.0)),
                        padding: UiRect::axes(Val::Px(40.0), Val::Px(28.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(14.0),
                        ..default()
                    },
                    BackgroundColor(NIGHT_SKY.with_alpha(0.92)),
                    BorderColor::all(Color::srgba(1.0, 0.85, 0.55, 0.6)),
                ))
                .with_children(|card| {
                    card.spawn(text(tr!(&format!("{id}-title")), 52.0, CREAM));
                    let playing = Seat::ALL.into_iter().filter(move |s| players.is_active(*s));
                    card.spawn(text(format_text(&format, playing), 20.0, DIM));
                    card.spawn(text(goal(id, &format), 28.0, CREAM));
                    card.spawn(text(tr!(&format!("{id}-how")), 20.0, DIM));
                    card.spawn(Node {
                        column_gap: Val::Px(28.0),
                        margin: UiRect::vertical(Val::Px(6.0)),
                        flex_wrap: FlexWrap::Wrap,
                        justify_content: JustifyContent::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        for (control, action) in controls(id) {
                            row.spawn(Node {
                                column_gap: Val::Px(8.0),
                                align_items: AlignItems::Center,
                                ..default()
                            })
                            .with_children(|pair| {
                                pair.spawn((
                                    Node {
                                        padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                                        border: UiRect::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BorderColor::all(CREAM),
                                ))
                                .with_child(text(control, 20.0, CREAM));
                                pair.spawn(text(action, 20.0, CREAM));
                            });
                        }
                    });
                    card.spawn(Node {
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|row| {
                        for seat in players.seats() {
                            row.spawn((
                                Node {
                                    width: Val::Px(150.0),
                                    padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                                BackgroundColor(
                                    Color::from(seat_color(seat) * 0.25).with_alpha(0.7),
                                ),
                            ))
                            .with_child((ReadyChip(seat), text(String::new(), 18.0, Color::WHITE)));
                        }
                    });
                    card.spawn(text(tr!("card-prompt"), 18.0, DIM));
                });
        })
        .id();
    commands.insert_resource(InstructionCard {
        ready,
        opened_at: time.elapsed_secs(),
        all_ready_at: None,
        root,
    });
}

fn update_card(
    mut commands: Commands,
    mut card: ResMut<InstructionCard>,
    seats: Res<Seats>,
    inputs: Res<SeatInputs>,
    time: Res<Time>,
    mut latch: ResMut<PressLatch>,
    mut chips: Query<(&ReadyChip, &mut Text)>,
) {
    let now = time.elapsed_secs();
    for seat in Seat::ALL {
        if !seats.is_human(seat) {
            continue;
        }
        // A seat whose pad is gone can't press anything; don't hold everyone else up.
        let lost = inputs.0.current[seat.index()].held(Button::CONTROLLER_LOST);
        if lost || inputs.just_pressed(seat, Button::A) {
            card.ready[seat.index()] = true;
        }
    }
    for (chip, mut label) in &mut chips {
        let status = if card.ready[chip.0.index()] {
            tr!("card-ready")
        } else {
            tr!("card-press-a")
        };
        label.0 = format!(
            "{}{}\n{status}",
            seat_label(chip.0),
            bot_tag(seats.is_bot(chip.0))
        );
    }

    let any_human = Seat::ALL.into_iter().any(|seat| seats.is_human(seat));
    if card.ready.iter().all(|r| *r) && card.all_ready_at.is_none() {
        card.all_ready_at = Some(now);
    }
    let close = match card.all_ready_at {
        Some(at) if any_human => now - at >= ALL_READY_SECS,
        _ => !any_human && now - card.opened_at >= UNATTENDED_SECS,
    };
    if close {
        // The A that closed the card belongs to the card, not the first tick of the game.
        latch.0 = [0; MAX_PLAYERS];
        commands.entity(card.root).despawn();
        commands.remove_resource::<InstructionCard>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minigames::SHOWN;

    #[test]
    fn every_minigame_has_a_card() {
        for id in SHOWN {
            for suffix in ["title", "goal", "how", "a"] {
                let key = format!("{id}-{suffix}");
                assert!(has_key(&key), "missing {key}");
            }
        }
    }

    #[test]
    fn quick_draw_goal_matches_the_rules() {
        let points = minigame_quick_draw::POINTS_TO_WIN.to_string();
        assert!(goal("quick-draw", &Format::FreeForAll).contains(&points));
    }

    #[test]
    fn the_countdown_counts_three_two_one() {
        let end = COUNTDOWN_TICKS as u32;
        assert_eq!(countdown(0), Some(3));
        assert_eq!(countdown(end - TICK_HZ - 1), Some(2));
        assert_eq!(countdown(end - TICK_HZ), Some(1));
        assert_eq!(countdown(end - 1), Some(1));
        assert_eq!(countdown(end), None);
    }

    #[test]
    fn team_formats_fall_back_to_the_plain_goal() {
        assert_eq!(
            goal(
                "quick-draw",
                &Format::TwoVsTwo {
                    team_of: [0, 0, 1, 1]
                }
            ),
            tr!("quick-draw-goal")
        );
    }
}
