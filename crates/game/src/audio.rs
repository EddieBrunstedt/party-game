//! Sound: every effect is synthesized at startup from a recipe below (`crate::sfx`) and played
//! through `bevy_kira_audio`, with a quiet night ambience looping underneath. Presentation
//! only; views decide when to play what.

use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_kira_audio::Audio;
use bevy_kira_audio::prelude::{
    AudioApp, AudioChannel, AudioControl, AudioPlugin as KiraPlugin, AudioSource,
};
use kira::Frame;
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};

use crate::scenery::noise;
use crate::sfx::{self, SAMPLE_RATE, Tone, Wave};

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        // Built here rather than at Startup: the first state's OnEnter can already play sounds.
        app.add_plugins(KiraPlugin)
            .add_audio_channel::<Ambience>()
            .init_resource::<Sounds>()
            .add_systems(Startup, start_ambience);
    }
}

/// The channel the night ambience loops on, so effects never fight it for volume.
#[derive(Resource)]
struct Ambience;

/// Every sound effect in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sound {
    // Menus and the instruction card.
    Move,
    Confirm,
    Back,
    Join,
    Ready,
    Count,
    Go,
    // Quick Draw.
    Heartbeat,
    Ignite,
    Draw,
    FalseStart,
    TooSlow,
    RoundWin,
    // Lantern Push.
    Dash,
    Hit,
    Brace,
    Rebound,
    Fall,
    Rumble,
    Tick,
    // The board.
    DiceStop,
    Step,
    CoinsUp,
    CoinsDown,
    Star,
    TurnStart,
    MinigameStart,
    // Anyone winning a minigame or the game.
    Win,
}

impl Sound {
    pub const ALL: [Sound; 28] = [
        Sound::Move,
        Sound::Confirm,
        Sound::Back,
        Sound::Join,
        Sound::Ready,
        Sound::Count,
        Sound::Go,
        Sound::Heartbeat,
        Sound::Ignite,
        Sound::Draw,
        Sound::FalseStart,
        Sound::TooSlow,
        Sound::RoundWin,
        Sound::Dash,
        Sound::Hit,
        Sound::Brace,
        Sound::Rebound,
        Sound::Fall,
        Sound::Rumble,
        Sound::Tick,
        Sound::DiceStop,
        Sound::Step,
        Sound::CoinsUp,
        Sound::CoinsDown,
        Sound::Star,
        Sound::TurnStart,
        Sound::MinigameStart,
        Sound::Win,
    ];

    /// The sound's samples: its tones, rendered to peak at a level that sets its loudness.
    pub fn render(self) -> Vec<f32> {
        use Wave::*;
        let t = Tone::new;
        let n = Tone::note;
        let (tones, peak): (Vec<Tone>, f32) = match self {
            Sound::Move => (vec![n(Pulse(0.5), 880.0, 0.04).decay(1.0)], 0.3),
            Sound::Confirm => (
                vec![
                    n(Pulse(0.25), 660.0, 0.06).decay(0.5),
                    n(Pulse(0.25), 990.0, 0.1).at(0.05),
                ],
                0.4,
            ),
            Sound::Back => (
                vec![
                    n(Pulse(0.25), 660.0, 0.06).decay(0.5),
                    n(Pulse(0.25), 440.0, 0.1).at(0.05),
                ],
                0.35,
            ),
            Sound::Join => (
                [C5, E5, G5, C6]
                    .iter()
                    .enumerate()
                    .flat_map(|(i, &f)| {
                        let at = i as f32 * 0.07;
                        let len = if i == 3 { 0.25 } else { 0.08 };
                        [
                            n(Triangle, f, len).at(at),
                            n(Pulse(0.5), f, len).at(at).volume(0.25),
                        ]
                    })
                    .collect(),
                0.45,
            ),
            Sound::Ready => (
                vec![
                    n(Triangle, 1319.0, 0.25).decay(3.0),
                    n(Sine, 2637.0, 0.15).decay(3.0).volume(0.3),
                ],
                0.4,
            ),
            Sound::Count => (
                vec![n(Pulse(0.5), 660.0, 0.14).decay(0.6).lowpass(4000.0)],
                0.45,
            ),
            Sound::Go => (
                vec![
                    n(Pulse(0.5), 1319.0, 0.4).lowpass(5000.0),
                    n(Pulse(0.5), 1976.0, 0.4).lowpass(5000.0).volume(0.5),
                    n(Triangle, 659.0, 0.4),
                ],
                0.5,
            ),
            Sound::Heartbeat => (
                vec![
                    t(Sine, 75.0, 45.0, 0.12).decay(2.0),
                    t(Sine, 70.0, 45.0, 0.12).decay(2.0).at(0.17).volume(0.7),
                    n(Noise, 400.0, 0.06).lowpass(160.0).volume(0.5),
                ],
                0.7,
            ),
            Sound::Ignite => (
                vec![
                    t(Noise, 3000.0, 300.0, 0.5)
                        .lowpass(2500.0)
                        .attack(0.01)
                        .decay(1.2)
                        .volume(0.8),
                    n(Sine, 98.0, 1.5).decay(2.5),
                    n(Sine, 196.0, 1.1).decay(3.0).volume(0.5),
                    n(Sine, 296.0, 0.8).decay(3.0).volume(0.3),
                    n(Triangle, 784.0, 0.7)
                        .attack(0.02)
                        .decay(2.0)
                        .vibrato(0.004, 6.0)
                        .volume(0.25),
                ],
                0.9,
            ),
            Sound::Draw => (
                vec![
                    t(Noise, 6000.0, 1500.0, 0.06).decay(2.0),
                    t(Pulse(0.3), 1800.0, 500.0, 0.09).volume(0.5),
                ],
                0.7,
            ),
            Sound::FalseStart => (
                vec![
                    n(Pulse(0.5), 110.0, 0.4).decay(0.3).vibrato(0.02, 8.0),
                    n(Saw, 117.0, 0.4).decay(0.3).lowpass(1800.0).volume(0.7),
                ],
                0.5,
            ),
            Sound::TooSlow => (
                vec![
                    t(Triangle, 392.0, 196.0, 0.45)
                        .decay(0.6)
                        .vibrato(0.03, 7.0),
                    t(Sine, 392.0, 196.0, 0.45).decay(0.6).volume(0.4),
                ],
                0.45,
            ),
            Sound::RoundWin => (
                [G5, B5, D6]
                    .iter()
                    .enumerate()
                    .flat_map(|(i, &f)| {
                        let at = i as f32 * 0.08;
                        let len = if i == 2 { 0.35 } else { 0.1 };
                        [
                            n(Triangle, f, len).at(at).decay(2.0),
                            n(Pulse(0.25), f, len).at(at).decay(2.0).volume(0.25),
                        ]
                    })
                    .collect(),
                0.5,
            ),
            Sound::Dash => (
                vec![
                    t(Noise, 800.0, 4000.0, 0.2)
                        .lowpass(3000.0)
                        .attack(0.03)
                        .decay(1.5),
                ],
                0.4,
            ),
            Sound::Hit => (
                vec![
                    t(Sine, 160.0, 45.0, 0.18).decay(2.5),
                    t(Noise, 3000.0, 800.0, 0.05).lowpass(5000.0).volume(0.6),
                    n(Pulse(0.5), 90.0, 0.06).volume(0.3),
                ],
                0.85,
            ),
            Sound::Brace => (
                vec![
                    n(Pulse(0.5), 180.0, 0.07).decay(2.0).lowpass(1200.0),
                    n(Noise, 1200.0, 0.04).lowpass(2000.0).volume(0.4),
                ],
                0.45,
            ),
            Sound::Rebound => (
                vec![
                    t(Triangle, 220.0, 520.0, 0.3)
                        .decay(1.2)
                        .vibrato(0.06, 14.0),
                    t(Sine, 110.0, 260.0, 0.3).decay(1.2).volume(0.5),
                ],
                0.6,
            ),
            Sound::Fall => (
                vec![t(Sine, 1200.0, 150.0, 0.9).decay(0.4).vibrato(0.01, 9.0)],
                0.45,
            ),
            Sound::Rumble => (
                vec![
                    n(Noise, 120.0, 1.2)
                        .lowpass(250.0)
                        .attack(0.1)
                        .decay(1.0)
                        .tremolo(0.5, 9.0),
                ],
                0.6,
            ),
            Sound::Tick => (
                vec![n(Pulse(0.5), 1760.0, 0.03).decay(2.0).lowpass(6000.0)],
                0.3,
            ),
            Sound::DiceStop => (
                vec![
                    n(Triangle, C6, 0.2).decay(2.0),
                    n(Triangle, G6, 0.3).at(0.06).decay(2.0),
                ],
                0.45,
            ),
            Sound::Step => (
                vec![
                    t(Sine, 330.0, 220.0, 0.06).decay(2.0),
                    n(Noise, 2000.0, 0.02).lowpass(4000.0).volume(0.3),
                ],
                0.35,
            ),
            Sound::CoinsUp => (
                vec![
                    n(Pulse(0.5), B5, 0.07).decay(0.2),
                    n(Pulse(0.5), E6, 0.3).at(0.07).decay(1.5),
                ],
                0.4,
            ),
            Sound::CoinsDown => (
                vec![
                    n(Pulse(0.5), E5, 0.08).lowpass(3000.0),
                    n(Pulse(0.5), C5, 0.08).at(0.08).lowpass(3000.0),
                    n(Pulse(0.5), G4, 0.18).at(0.16).decay(1.5).lowpass(3000.0),
                ],
                0.4,
            ),
            Sound::Star => (
                [C6, E6, G6, C7]
                    .iter()
                    .enumerate()
                    .map(|(i, &f)| n(Triangle, f, 0.3).at(i as f32 * 0.06).decay(2.0))
                    .chain([
                        n(Sine, 3136.0, 0.8).at(0.2).tremolo(0.8, 20.0).volume(0.3),
                        n(Triangle, C5, 0.8).decay(1.5).volume(0.4),
                        n(Triangle, G5, 0.8).decay(1.5).volume(0.3),
                    ])
                    .collect(),
                0.55,
            ),
            Sound::TurnStart => (
                vec![
                    n(Triangle, A5, 0.3).decay(2.5),
                    n(Sine, 1760.0, 0.2).decay(2.5).volume(0.3),
                ],
                0.35,
            ),
            Sound::MinigameStart => (
                vec![
                    t(Noise, 400.0, 5000.0, 0.35)
                        .lowpass(4000.0)
                        .attack(0.1)
                        .decay(0.5)
                        .volume(0.6),
                    n(Pulse(0.25), C5, 0.1).at(0.3),
                    n(Pulse(0.25), G5, 0.3).at(0.4),
                ],
                0.5,
            ),
            Sound::Win => {
                let lead = [C5, E5, G5].iter().enumerate().map(|(i, &f)| {
                    n(Pulse(0.5), f, 0.12)
                        .at(i as f32 * 0.12)
                        .decay(0.5)
                        .lowpass(5000.0)
                });
                let last = [
                    n(Pulse(0.5), C6, 0.6)
                        .at(0.36)
                        .decay(1.5)
                        .vibrato(0.006, 6.0)
                        .lowpass(5000.0),
                    n(Triangle, E5, 0.6).at(0.36).decay(1.5).volume(0.5),
                    n(Triangle, 130.8, 0.36).decay(0.5),
                    n(Triangle, 196.0, 0.6).at(0.36).decay(1.5),
                ];
                (lead.chain(last).collect(), 0.55)
            }
        };
        sfx::render(&tones, peak)
    }
}

// Note pitches in Hz.
const G4: f32 = 392.0;
const C5: f32 = 523.25;
const E5: f32 = 659.25;
const G5: f32 = 783.99;
const A5: f32 = 880.0;
const B5: f32 = 987.77;
const C6: f32 = 1046.5;
const D6: f32 = 1174.66;
const E6: f32 = 1318.51;
const G6: f32 = 1567.98;
const C7: f32 = 2093.0;

/// A seamless 12 s loop of crickets over a soft wind.
pub fn night_ambience() -> Vec<f32> {
    let length = 12.0;
    let mut tones = vec![
        Tone::note(Wave::Noise, 900.0, length)
            .lowpass(350.0)
            .attack(0.0)
            .decay(0.0)
            .tremolo(0.6, 0.13)
            .volume(0.35),
    ];
    // Two crickets at slightly different pitches, each chirping in little trills.
    for (cricket, pitch) in [(0, 4400.0), (1, 4950.0)] {
        let mut at = 0.2 + cricket as f32 * 0.45;
        let mut k = 0;
        while at < length - 0.4 {
            tones.push(
                Tone::note(Wave::Sine, pitch, 0.13)
                    .at(at)
                    .attack(0.01)
                    .decay(0.8)
                    .tremolo(1.0, 32.0)
                    .volume(0.14),
            );
            at += 0.7 + noise(k, cricket, 91) * 0.9;
            k += 1;
        }
    }
    let mut samples = sfx::render(&tones, 0.5);
    samples.truncate((length * SAMPLE_RATE as f32) as usize);
    // Fade both ends a touch so the wind doesn't click where the loop wraps.
    let fade = (0.02 * SAMPLE_RATE as f32) as usize;
    let n = samples.len();
    for i in 0..fade {
        let k = i as f32 / fade as f32;
        samples[i] *= k;
        samples[n - 1 - i] *= k;
    }
    samples
}

fn source(samples: &[f32]) -> AudioSource {
    AudioSource {
        sound: StaticSoundData {
            sample_rate: SAMPLE_RATE,
            frames: samples
                .iter()
                .map(|s| Frame::from_mono(*s))
                .collect::<Arc<[_]>>(),
            settings: StaticSoundSettings::default(),
            slice: None,
        },
    }
}

#[derive(Resource)]
pub struct Sounds {
    effects: Vec<Handle<AudioSource>>,
    ambience: Handle<AudioSource>,
}

impl FromWorld for Sounds {
    fn from_world(world: &mut World) -> Self {
        let mut sources = world.resource_mut::<Assets<AudioSource>>();
        Self {
            effects: Sound::ALL
                .iter()
                .map(|s| sources.add(source(&s.render())))
                .collect(),
            ambience: sources.add(source(&night_ambience())),
        }
    }
}

fn start_ambience(channel: Res<AudioChannel<Ambience>>, sounds: Res<Sounds>) {
    channel.play(sounds.ambience.clone()).looped();
    channel.set_volume(-14.0);
}

/// Plays sound effects from any system.
#[derive(SystemParam)]
pub struct Sfx<'w> {
    audio: Res<'w, Audio>,
    sounds: Res<'w, Sounds>,
}

impl Sfx<'_> {
    pub fn play(&self, sound: Sound) {
        self.play_with(sound, 1.0, 0.0);
    }

    /// For sounds that repeat a lot (steps, hits): pitched a little differently per `variant`.
    pub fn play_varied(&self, sound: Sound, variant: u32) {
        let rate = 0.92 + 0.16 * noise(variant as i32, 0, 37) as f64;
        self.play_with(sound, rate, 0.0);
    }

    /// `rate` scales speed and pitch together; `volume` is in decibels.
    pub fn play_with(&self, sound: Sound, rate: f64, volume: f32) {
        let index = Sound::ALL.iter().position(|s| *s == sound).unwrap_or(0);
        self.audio
            .play(self.sounds.effects[index].clone())
            .with_playback_rate(rate)
            .with_volume(volume);
    }
}

/// Writes every sound and the ambience to `dir` as WAVs (`--dump-sfx`).
pub fn dump(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for sound in Sound::ALL {
        let name = format!("{sound:?}").to_lowercase();
        std::fs::write(dir.join(format!("{name}.wav")), sfx::wav(&sound.render()))?;
    }
    std::fs::write(dir.join("ambience.wav"), sfx::wav(&night_ambience()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_renders_short_finite_and_within_range() {
        for sound in Sound::ALL {
            let samples = sound.render();
            let secs = samples.len() as f32 / SAMPLE_RATE as f32;
            assert!((0.03..2.0).contains(&secs), "{sound:?} lasts {secs} s");
            assert!(
                samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
                "{sound:?} clips"
            );
        }
        let ambience = night_ambience();
        assert_eq!(ambience.len(), 12 * SAMPLE_RATE as usize);
    }

    #[test]
    fn the_list_has_every_sound_once() {
        let mut seen = std::collections::HashSet::new();
        assert!(Sound::ALL.iter().all(|s| seen.insert(*s)));
    }
}
