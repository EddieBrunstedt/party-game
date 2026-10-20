//! The playable lantern folk. One shared body; each character's personality is its light
//! source (DESIGN.md §3.5). Each is a `.glb` built by `art/lantern_folk.py` on one shared rig
//! with the same animations; the `Body`, `Glow` and `Accent` materials take the player's colours.

use std::time::Duration;

use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldInstanceReady};
use sim_core::{MAX_PLAYERS, Seat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Character {
    PaperLantern,
    FireflyJar,
    Candle,
    NeonRing,
}

impl Character {
    pub const ALL: [Character; 4] = [
        Character::PaperLantern,
        Character::FireflyJar,
        Character::Candle,
        Character::NeonRing,
    ];

    /// Fluent key of the character's name.
    pub fn name_key(self) -> &'static str {
        match self {
            Character::PaperLantern => "character-paper-lantern",
            Character::FireflyJar => "character-firefly-jar",
            Character::Candle => "character-candle",
            Character::NeonRing => "character-neon-ring",
        }
    }

    pub fn name(self) -> String {
        crate::tr!(self.name_key())
    }

    /// The character's model, under `assets/`.
    pub fn model_path(self) -> &'static str {
        match self {
            Character::PaperLantern => "characters/paper-lantern.glb",
            Character::FireflyJar => "characters/firefly-jar.glb",
            Character::Candle => "characters/candle.glb",
            Character::NeonRing => "characters/neon-ring.glb",
        }
    }
}

/// Who plays which character, by seat. Bots get whoever is left.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Roster(pub [Character; MAX_PLAYERS]);

impl Default for Roster {
    fn default() -> Self {
        Self(Character::ALL)
    }
}

impl Roster {
    pub fn of(&self, seat: Seat) -> Character {
        self.0[seat.index()]
    }
}

/// What a character's body is doing. Views set it on the character's root; the model plays
/// the matching animation, blending from the last one.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pose {
    #[default]
    Idle,
    /// One hop straight up, [`HOP_SECONDS`] long, then it stands still. Setting it again
    /// after another pose hops again.
    Hop,
    /// Bouncing with both arms up.
    Celebrate,
    /// Hunched over, head hanging.
    Slump,
}

/// How long [`Pose::Hop`] takes.
pub const HOP_SECONDS: f32 = 0.4;
/// How long one animation takes to blend into the next.
const BLEND: Duration = Duration::from_millis(150);

impl Pose {
    pub const ALL: [Pose; 4] = [Pose::Idle, Pose::Hop, Pose::Celebrate, Pose::Slump];

    /// The animation's name in the models.
    pub fn clip(self) -> &'static str {
        match self {
            Pose::Idle => "idle",
            Pose::Hop => "hop",
            Pose::Celebrate => "celebrate",
            Pose::Slump => "slump",
        }
    }
}

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        // Built here rather than at Startup: the first state's OnEnter spawns characters.
        app.init_resource::<CharacterModels>()
            .add_observer(load_model)
            .add_observer(dress_model)
            .add_systems(Update, play_poses);
    }
}

/// The four models, loading from startup, and each one's animations once it has loaded.
#[derive(Resource)]
pub struct CharacterModels {
    gltfs: [Handle<Gltf>; 4],
    scenes: [Handle<WorldAsset>; 4],
    clips: [Option<Clips>; 4],
}

impl FromWorld for CharacterModels {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            gltfs: Character::ALL.map(|c| assets.load(c.model_path())),
            scenes: Character::ALL
                .map(|c| assets.load(GltfAssetLabel::Scene(0).from_asset(c.model_path()))),
            clips: Default::default(),
        }
    }
}

/// One model's animation graph, with a node per [`Pose`].
#[derive(Clone)]
struct Clips {
    graph: Handle<AnimationGraph>,
    nodes: [AnimationNodeIndex; 4],
}

impl CharacterModels {
    /// `character`'s animations, building the graph the first time.
    fn clips(
        &mut self,
        character: Character,
        gltfs: &Assets<Gltf>,
        graphs: &mut Assets<AnimationGraph>,
    ) -> Option<Clips> {
        let slot = &mut self.clips[character as usize];
        if slot.is_none() {
            let gltf = gltfs.get(&self.gltfs[character as usize])?;
            let clips = Pose::ALL.map(|pose| gltf.named_animations.get(pose.clip()).cloned());
            let Some(clips) = clips.into_iter().collect::<Option<Vec<_>>>() else {
                error!("{} is missing an animation", character.model_path());
                return None;
            };
            let (graph, nodes) = AnimationGraph::from_clips(clips);
            *slot = Some(Clips {
                graph: graphs.add(graph),
                nodes: nodes.try_into().ok()?,
            });
        }
        slot.clone()
    }
}

/// Puts `character`'s model on this entity, which should sit at the feet facing +z. The model
/// spawns as a child once loaded, wearing these materials in place of its own.
#[derive(Component)]
pub struct CharacterModel {
    pub character: Character,
    pub body: Handle<StandardMaterial>,
    pub glow: Handle<StandardMaterial>,
    pub accent: Handle<StandardMaterial>,
    /// A child of this entity, moved onto the head bone so it follows the head around.
    pub head_light: Entity,
}

/// The spawned model's animation player, and the pose it's playing.
#[derive(Component)]
struct Rig {
    player: Entity,
    playing: Pose,
}

fn load_model(
    add: On<Add<CharacterModel>>,
    mut commands: Commands,
    models: Res<CharacterModels>,
    owners: Query<&CharacterModel>,
) {
    let Ok(model) = owners.get(add.entity) else {
        return;
    };
    let scene = models.scenes[model.character as usize].clone();
    commands.entity(add.entity).insert(WorldAssetRoot(scene));
}

#[allow(clippy::too_many_arguments)]
fn dress_model(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    mut models: ResMut<CharacterModels>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    owners: Query<(&CharacterModel, &Pose)>,
    children: Query<&Children>,
    nodes: Query<(Option<&Name>, Option<&GltfMaterialName>)>,
    hierarchy: Query<(&Transform, &ChildOf)>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let root = ready.entity;
    let Ok((model, pose)) = owners.get(root) else {
        return;
    };
    let clips = models.clips(model.character, &gltfs, &mut graphs);
    for node in children.iter_descendants(root) {
        let Ok((name, material)) = nodes.get(node) else {
            continue;
        };
        let swap = match material.map(|m| m.0.as_str()) {
            Some("Body") => Some(&model.body),
            Some("Glow") => Some(&model.glow),
            Some("Accent") => Some(&model.accent),
            _ => None,
        };
        if let Some(swap) = swap {
            commands.entity(node).insert(MeshMaterial3d(swap.clone()));
        }
        if name.is_some_and(|n| n.as_str() == "head")
            && let Ok((light, _)) = hierarchy.get(model.head_light)
        {
            // Same place relative to the root as before, but now it rides along with the head.
            let head = offset_from(root, node, &hierarchy);
            let light = Transform::from_matrix(head.to_matrix().inverse() * light.to_matrix());
            commands
                .entity(model.head_light)
                .insert((ChildOf(node), light));
        }
        if let (Ok(mut player), Some(clips)) = (players.get_mut(node), &clips) {
            let mut transitions = AnimationTransitions::new();
            play(&mut transitions, &mut player, clips, *pose, Duration::ZERO);
            commands
                .entity(node)
                .insert((AnimationGraphHandle(clips.graph.clone()), transitions));
            commands.entity(root).insert(Rig {
                player: node,
                playing: *pose,
            });
        }
    }
}

/// Where `node` sits relative to its ancestor `root`.
fn offset_from(
    root: Entity,
    mut node: Entity,
    hierarchy: &Query<(&Transform, &ChildOf)>,
) -> Transform {
    let mut offset = Transform::IDENTITY;
    while node != root {
        let Ok((transform, child_of)) = hierarchy.get(node) else {
            break;
        };
        offset = transform.mul_transform(offset);
        node = child_of.parent();
    }
    offset
}

fn play(
    transitions: &mut AnimationTransitions,
    player: &mut AnimationPlayer,
    clips: &Clips,
    pose: Pose,
    blend: Duration,
) {
    let animation = transitions.play(player, clips.nodes[pose as usize], blend);
    if pose != Pose::Hop {
        animation.repeat();
    }
}

fn play_poses(
    models: Res<CharacterModels>,
    mut folk: Query<(&CharacterModel, &Pose, &mut Rig)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (model, pose, mut rig) in &mut folk {
        if rig.playing == *pose {
            continue;
        }
        let Some(clips) = &models.clips[model.character as usize] else {
            continue;
        };
        if let Ok((mut player, mut transitions)) = players.get_mut(rig.player) {
            play(&mut transitions, &mut player, clips, *pose, BLEND);
            rig.playing = *pose;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON chunk of a `.glb`.
    fn gltf_json(path: &str) -> String {
        let full = format!("{}/assets/{path}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&full).unwrap_or_else(|e| panic!("{full}: {e}"));
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        String::from_utf8(bytes[20..20 + len].to_vec()).unwrap()
    }

    #[test]
    fn every_model_has_every_pose_and_the_player_colour_materials() {
        for character in Character::ALL {
            let json = gltf_json(character.model_path());
            let names = Pose::ALL
                .map(|p| p.clip())
                .into_iter()
                .chain(["Body", "Glow", "head"]);
            for name in names {
                assert!(
                    json.contains(&format!("\"name\":\"{name}\"")),
                    "{} has no {name}",
                    character.model_path()
                );
            }
        }
    }
}
