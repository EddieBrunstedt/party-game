//! Frame-time overlay (F3 toggles). The Steam Deck budget is enforced by watching this number.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

pub struct PerfOverlayPlugin;

impl Plugin for PerfOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, (toggle_overlay, update_overlay));
    }
}

#[derive(Component)]
struct PerfText;

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        PerfText,
        Text::new(""),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgba(0.8, 0.85, 1.0, 0.7)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(10.0),
            ..default()
        },
    ));
}

fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay: Query<&mut Visibility, With<PerfText>>,
) {
    if keys.just_pressed(KeyCode::F3) {
        for mut visibility in &mut overlay {
            visibility.toggle_visible_hidden();
        }
    }
}

fn update_overlay(
    diagnostics: Res<DiagnosticsStore>,
    mut overlay: Query<&mut Text, With<PerfText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed());
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed());
    if let (Some(fps), Some(frame_ms)) = (fps, frame_ms) {
        for mut text in &mut overlay {
            text.0 = format!("{fps:.0} fps  {frame_ms:.2} ms");
        }
    }
}
