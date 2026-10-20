//! Quality presets (DESIGN.md §2). "Deck" targets 60 fps on a Steam Deck at 1280×800: cheaper
//! shadows, no MSAA, a smaller bloom chain and no point-light shadows. F4 (or the pad's
//! Select button) toggles it in-game; `--quality deck` starts with it.

use bevy::input::gamepad::GamepadButton;
use bevy::light::{
    CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap, PointLightShadowMap,
};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::window::WindowResolution;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Quality {
    #[default]
    Desktop,
    Deck,
}

pub struct QualityPlugin(pub Quality);

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0).add_systems(
            Update,
            (toggle_quality, apply_window, apply_lights, apply_cameras).chain(),
        );
    }
}

/// Remembers whether a point light was built to cast shadows, so Desktop can restore it.
#[derive(Component)]
struct WantsShadows(bool);

fn toggle_quality(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut quality: ResMut<Quality>,
) {
    let pressed = keys.just_pressed(KeyCode::F4)
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::Select));
    if pressed {
        *quality = match *quality {
            Quality::Desktop => Quality::Deck,
            Quality::Deck => Quality::Desktop,
        };
        info!("quality preset: {:?}", *quality);
    }
}

fn apply_window(quality: Res<Quality>, mut windows: Query<&mut Window>) {
    if !quality.is_changed() || *quality != Quality::Deck {
        return;
    }
    // The Deck's own screen size.
    for mut window in &mut windows {
        window.resolution = WindowResolution::new(1280, 800);
    }
}

fn apply_lights(
    mut commands: Commands,
    quality: Res<Quality>,
    mut directional: ResMut<DirectionalLightShadowMap>,
    mut point_map: ResMut<PointLightShadowMap>,
    mut suns: Query<(Ref<DirectionalLight>, &mut CascadeShadowConfig)>,
    mut points: Query<(Entity, &mut PointLight, Option<&WantsShadows>)>,
) {
    let deck = *quality == Quality::Deck;
    if quality.is_changed() {
        directional.size = if deck { 1024 } else { 2048 };
        point_map.size = if deck { 256 } else { 1024 };
    }
    for (light, mut cascades) in &mut suns {
        if quality.is_changed() || light.is_added() {
            // Our scenes are small: a couple of cascades over ~40 units covers them.
            *cascades = CascadeShadowConfigBuilder {
                num_cascades: if deck { 1 } else { 2 },
                maximum_distance: 40.0,
                first_cascade_far_bound: if deck { 40.0 } else { 15.0 },
                ..default()
            }
            .build();
        }
    }
    for (entity, mut light, wants) in &mut points {
        let wanted = match wants {
            Some(wants) => wants.0,
            None => {
                commands
                    .entity(entity)
                    .insert(WantsShadows(light.shadow_maps_enabled));
                light.shadow_maps_enabled
            }
        };
        let enabled = wanted && !deck;
        if light.shadow_maps_enabled != enabled {
            light.shadow_maps_enabled = enabled;
        }
    }
}

fn apply_cameras(
    quality: Res<Quality>,
    mut cameras: Query<(Ref<Camera3d>, &mut Msaa, Option<&mut Bloom>)>,
) {
    let deck = *quality == Quality::Deck;
    for (camera, mut msaa, bloom) in &mut cameras {
        if !(quality.is_changed() || camera.is_added()) {
            continue;
        }
        *msaa = if deck { Msaa::Off } else { Msaa::Sample4 };
        if let Some(mut bloom) = bloom {
            bloom.max_mip_dimension = if deck { 256 } else { 512 };
        }
    }
}
