//! Frame-time overlay (F3 toggles). The Steam Deck budget is enforced by watching these numbers:
//! GPU time comes from timestamp queries around each render pass (Vulkan and DX12 only).

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;

use crate::quality::Quality;

pub struct PerfOverlayPlugin {
    /// Print a GPU frame-time summary when the app exits.
    pub report: bool,
}

impl Plugin for PerfOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            RenderDiagnosticsPlugin,
        ))
        .init_resource::<GpuStats>()
        .add_systems(Startup, spawn_overlay)
        .add_systems(Update, (toggle_overlay, track_gpu, update_overlay).chain());
        if self.report {
            app.add_systems(Last, report_on_exit);
        }
    }
}

/// GPU time per frame, summed over the top-level render spans.
#[derive(Resource, Default)]
struct GpuStats {
    latest: Option<f64>,
    /// Frames measured after the warm-up, for the exit report.
    samples: Vec<f64>,
}

/// Seconds to skip before sampling, while shaders compile and the scene settles.
const WARM_UP_SECS: f32 = 4.0;

fn track_gpu(
    time: Res<Time<Real>>,
    diagnostics: Res<DiagnosticsStore>,
    mut stats: ResMut<GpuStats>,
) {
    // Paths look like `render/<span>/elapsed_gpu`; nested spans have more components and are
    // already inside their parent's time.
    let total: f64 = diagnostics
        .iter()
        .filter(|d| {
            let parts: Vec<&str> = d.path().components().collect();
            parts.len() == 3 && parts[0] == "render" && parts[2] == "elapsed_gpu"
        })
        .filter_map(|d| d.value())
        .sum();
    if total > 0.0 {
        stats.latest = Some(total);
        if time.elapsed_secs() > WARM_UP_SECS {
            stats.samples.push(total);
        }
    }
}

fn report_on_exit(
    mut exits: MessageReader<AppExit>,
    stats: Res<GpuStats>,
    quality: Res<Quality>,
    diagnostics: Res<DiagnosticsStore>,
) {
    if exits.read().next().is_none() {
        return;
    }
    if stats.samples.is_empty() {
        println!("gpu-report: no GPU timings (timestamp queries unsupported?)");
        return;
    }
    let mut sorted = stats.samples.clone();
    sorted.sort_by(f64::total_cmp);
    let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
    let max = sorted[sorted.len() - 1];
    println!(
        "gpu-report: {:?} preset, {} frames: avg {avg:.2} ms, p95 {p95:.2} ms, max {max:.2} ms",
        *quality,
        sorted.len()
    );
    // Where the time goes, biggest first (last frame). Particle compute isn't timed.
    let mut spans: Vec<(String, f64)> = diagnostics
        .iter()
        .filter_map(|d| {
            let parts: Vec<&str> = d.path().components().collect();
            (parts.len() == 3 && parts[2] == "elapsed_gpu")
                .then(|| (parts[1].to_string(), d.value().unwrap_or(0.0)))
        })
        .collect();
    spans.sort_by(|a, b| b.1.total_cmp(&a.1));
    let spans: Vec<String> = spans
        .iter()
        .map(|(name, ms)| format!("{name} {ms:.2}"))
        .collect();
    println!("gpu-report spans (ms): {}", spans.join(", "));
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
        // Above every screen's own UI.
        GlobalZIndex(10),
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
    stats: Res<GpuStats>,
    quality: Res<Quality>,
    mut overlay: Query<&mut Text, With<PerfText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed());
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed());
    if let (Some(fps), Some(frame_ms)) = (fps, frame_ms) {
        let gpu = stats
            .latest
            .map(|ms| format!("  gpu {ms:.2} ms"))
            .unwrap_or_default();
        for mut text in &mut overlay {
            text.0 = format!("{fps:.0} fps  {frame_ms:.2} ms{gpu}  [{:?}]", *quality);
        }
    }
}
