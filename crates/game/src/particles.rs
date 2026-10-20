//! GPU particles (`bevy_hanabi`): a few shared effect assets that any scene can fire.
//! Presentation only; nothing here touches simulation state.
//!
//! Budget (DESIGN.md §2): every effect has a fixed particle capacity, so the worst case is
//! known up front. Bursts are one-shot entities that clean themselves up.

use bevy::prelude::*;
use bevy_hanabi::prelude::*;

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        // Created at build time, not in `Startup`: the first state's `OnEnter` runs before it.
        app.add_plugins(HanabiPlugin)
            .init_resource::<Effects>()
            .add_systems(Update, despawn_expired);
    }
}

const BURST_SECS: f32 = 0.7;

#[derive(Resource)]
pub struct Effects {
    /// 40 coloured sparks flying out (colour via [`burst`]).
    burst: Handle<EffectAsset>,
    /// A short glowing trail behind a moving emitter; colour via the "color" property.
    pub trail: Handle<EffectAsset>,
    /// Slow motes drifting in a 20 x 3 x 14 box around the emitter.
    pub motes: Handle<EffectAsset>,
}

/// Despawned once its particles are gone.
#[derive(Component)]
struct Expires(f32);

/// An HDR colour as the `Vec4` effect properties expect. Values above 1 bloom.
pub fn hdr(color: LinearRgba, strength: f32) -> Vec4 {
    let c = color * strength;
    Vec4::new(c.red, c.green, c.blue, 1.0)
}

/// Fire a one-shot burst of sparks at `pos`.
pub fn burst(
    commands: &mut Commands,
    effects: &Effects,
    scope: impl Component,
    pos: Vec3,
    color: Vec4,
) {
    commands.spawn((
        scope,
        ParticleEffect::new(effects.burst.clone()),
        EffectProperties::default().with_properties([("color".to_string(), color.into())]),
        Transform::from_translation(pos),
        Expires(BURST_SECS + 0.5),
    ));
}

/// Properties for an effect with a "color" property.
pub fn colored(color: Vec4) -> EffectProperties {
    EffectProperties::default().with_properties([("color".to_string(), color.into())])
}

fn despawn_expired(
    mut commands: Commands,
    time: Res<Time>,
    mut expiring: Query<(Entity, &mut Expires)>,
) {
    for (entity, mut expires) in &mut expiring {
        expires.0 -= time.delta_secs();
        if expires.0 <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn fade_out() -> bevy_hanabi::Gradient<Vec4> {
    let mut fade = bevy_hanabi::Gradient::new();
    fade.add_key(0.0, Vec4::ONE);
    fade.add_key(0.5, Vec4::ONE);
    fade.add_key(1.0, Vec4::new(1.0, 1.0, 1.0, 0.0));
    fade
}

fn burst_asset() -> EffectAsset {
    let w = ExprWriter::new();
    let color = w.add_property("color", Vec4::ONE.into());
    let init_color = SetAttributeModifier::new(Attribute::HDR_COLOR, w.prop(color).expr());
    let init_pos = SetPositionSphereModifier {
        center: w.lit(Vec3::ZERO).expr(),
        radius: w.lit(0.05).expr(),
        dimension: ShapeDimension::Volume,
    };
    let init_vel = SetVelocitySphereModifier {
        center: w.lit(Vec3::ZERO).expr(),
        speed: w.lit(2.5).uniform(w.lit(5.5)).expr(),
    };
    let init_age = SetAttributeModifier::new(Attribute::AGE, w.lit(0.0).expr());
    let init_life = SetAttributeModifier::new(Attribute::LIFETIME, w.lit(BURST_SECS).expr());
    let drag = LinearDragModifier::new(w.lit(4.0).expr());
    let gravity = AccelModifier::new(w.lit(Vec3::new(0.0, -5.0, 0.0)).expr());
    let mut size = bevy_hanabi::Gradient::new();
    size.add_key(0.0, Vec3::new(0.22, 0.05, 0.05));
    size.add_key(1.0, Vec3::new(0.04, 0.02, 0.02));
    EffectAsset::new(64, SpawnerSettings::once(40.0.into()), w.finish())
        .with_name("burst")
        .with_alpha_mode(bevy_hanabi::AlphaMode::Add)
        .init(init_pos)
        .init(init_vel)
        .init(init_age)
        .init(init_life)
        .init(init_color)
        .update(drag)
        .update(gravity)
        .render(ColorOverLifetimeModifier {
            gradient: fade_out(),
            blend: ColorBlendMode::Modulate,
            mask: ColorBlendMask::RGBA,
        })
        .render(SizeOverLifetimeModifier {
            gradient: size,
            screen_space_size: false,
        })
        .render(OrientModifier::new(OrientMode::AlongVelocity))
}

fn trail_asset() -> EffectAsset {
    let w = ExprWriter::new();
    let color = w.add_property("color", Vec4::ONE.into());
    let init_color = SetAttributeModifier::new(Attribute::HDR_COLOR, w.prop(color).expr());
    let init_pos = SetPositionSphereModifier {
        center: w.lit(Vec3::ZERO).expr(),
        radius: w.lit(0.08).expr(),
        dimension: ShapeDimension::Volume,
    };
    let init_vel = SetAttributeModifier::new(
        Attribute::VELOCITY,
        ((w.rand(VectorType::VEC3F) * w.lit(2.0) - w.lit(1.0)) * w.lit(0.2)).expr(),
    );
    let init_age = SetAttributeModifier::new(Attribute::AGE, w.lit(0.0).expr());
    let init_life =
        SetAttributeModifier::new(Attribute::LIFETIME, w.lit(0.35).uniform(w.lit(0.6)).expr());
    let roundness = w.lit(1.0).expr();
    let mut size = bevy_hanabi::Gradient::new();
    size.add_key(0.0, Vec3::splat(0.09));
    size.add_key(1.0, Vec3::splat(0.0));
    EffectAsset::new(48, SpawnerSettings::rate(45.0.into()), w.finish())
        .with_name("trail")
        .with_alpha_mode(bevy_hanabi::AlphaMode::Add)
        .init(init_pos)
        .init(init_vel)
        .init(init_age)
        .init(init_life)
        .init(init_color)
        .render(SizeOverLifetimeModifier {
            gradient: size,
            screen_space_size: false,
        })
        .render(OrientModifier::new(OrientMode::FaceCameraPosition))
        .render(RoundModifier { roundness })
}

fn motes_asset() -> EffectAsset {
    let w = ExprWriter::new();
    let init_pos = SetPositionBoxModifier {
        center: w.lit(Vec3::new(0.0, 1.5, 0.0)).expr(),
        extent: w.lit(Vec3::new(20.0, 3.0, 14.0)).expr(),
        dimension: ShapeDimension::Volume,
    };
    let drift = (w.rand(VectorType::VEC3F) * w.lit(2.0) - w.lit(1.0)) * w.lit(0.15)
        + w.lit(Vec3::new(0.0, 0.06, 0.0));
    let init_vel = SetAttributeModifier::new(Attribute::VELOCITY, drift.expr());
    let init_age = SetAttributeModifier::new(Attribute::AGE, w.lit(0.0).expr());
    let init_life =
        SetAttributeModifier::new(Attribute::LIFETIME, w.lit(4.0).uniform(w.lit(6.0)).expr());
    let init_color = SetAttributeModifier::new(
        Attribute::HDR_COLOR,
        w.lit(Vec4::new(2.0, 2.6, 0.9, 1.0)).expr(),
    );
    let mut fade = bevy_hanabi::Gradient::new();
    fade.add_key(0.0, Vec4::new(1.0, 1.0, 1.0, 0.0));
    fade.add_key(0.2, Vec4::ONE);
    fade.add_key(0.8, Vec4::ONE);
    fade.add_key(1.0, Vec4::new(1.0, 1.0, 1.0, 0.0));
    let roundness = w.lit(1.0).expr();
    EffectAsset::new(256, SpawnerSettings::rate(18.0.into()), w.finish())
        .with_name("motes")
        .with_alpha_mode(bevy_hanabi::AlphaMode::Add)
        .init(init_pos)
        .init(init_vel)
        .init(init_age)
        .init(init_life)
        .init(init_color)
        .render(ColorOverLifetimeModifier {
            gradient: fade,
            blend: ColorBlendMode::Modulate,
            mask: ColorBlendMask::RGBA,
        })
        .render(SetSizeModifier {
            size: Vec3::splat(0.05).into(),
        })
        .render(OrientModifier::new(OrientMode::FaceCameraPosition))
        .render(RoundModifier { roundness })
}

impl FromWorld for Effects {
    fn from_world(world: &mut World) -> Self {
        let mut assets = world.resource_mut::<Assets<EffectAsset>>();
        Self {
            burst: assets.add(burst_asset()),
            trail: assets.add(trail_asset()),
            motes: assets.add(motes_asset()),
        }
    }
}
