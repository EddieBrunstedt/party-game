//! Development helpers.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// Saves screenshots at fixed times after startup, then exits. Lets visuals be checked
/// without a human at the keyboard.
pub struct ScreenshotPlugin {
    pub at_seconds: Vec<f32>,
    pub dir: PathBuf,
}

#[derive(Resource)]
struct ScreenshotSchedule {
    pending: Vec<f32>,
    dir: PathBuf,
    taken: usize,
    exit_at: f32,
}

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        let mut pending = self.at_seconds.clone();
        pending.sort_by(f32::total_cmp);
        let exit_at = pending.last().copied().unwrap_or(0.0) + 1.0;
        app.insert_resource(ScreenshotSchedule {
            pending,
            dir: self.dir.clone(),
            taken: 0,
            exit_at,
        })
        .add_systems(Update, take_screenshots);
    }
}

fn take_screenshots(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut schedule: ResMut<ScreenshotSchedule>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs();
    while schedule.pending.first().is_some_and(|at| *at <= now) {
        schedule.pending.remove(0);
        schedule.taken += 1;
        let _ = std::fs::create_dir_all(&schedule.dir);
        let path = schedule.dir.join(format!("shot-{}.png", schedule.taken));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if schedule.pending.is_empty() && now >= schedule.exit_at {
        exit.write(AppExit::Success);
    }
}
