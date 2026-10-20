//! A tiny sfxr-style synthesizer: every sound effect is a few [`Tone`]s rendered to samples at
//! startup, so the game ships no audio files and every sound is the same on every run.
//! `--dump-sfx <dir>` writes them all as WAVs for listening. Plain Rust; `audio.rs` plays them.

use std::f32::consts::TAU;

pub const SAMPLE_RATE: u32 = 44_100;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wave {
    Sine,
    Triangle,
    Saw,
    /// A pulse wave with this duty cycle (0.5 is a square).
    Pulse(f32),
    /// Noise re-sampled at the tone's frequency, so pitch still shapes it (as in sfxr).
    Noise,
}

/// One voice of a sound: a wave gliding between two pitches under a simple envelope.
#[derive(Debug, Clone, Copy)]
pub struct Tone {
    pub wave: Wave,
    /// When it starts within the sound, in seconds.
    pub at: f32,
    /// How long it lasts, in seconds.
    pub length: f32,
    /// Pitch at the start and end, in Hz; the glide is exponential.
    pub from: f32,
    pub to: f32,
    /// Seconds to fade in.
    pub attack: f32,
    /// How fast it dies away: 1 fades linearly, higher is more percussive, 0 holds then cuts.
    pub decay: f32,
    pub volume: f32,
    /// Vibrato depth (fraction of the pitch) and rate (Hz).
    pub vibrato: (f32, f32),
    /// One-pole low-pass cutoff in Hz; 0 leaves it open.
    pub lowpass: f32,
    /// Amplitude wobble depth (0..1) and rate (Hz), for trills and flutter.
    pub tremolo: (f32, f32),
}

impl Tone {
    pub const fn new(wave: Wave, from: f32, to: f32, length: f32) -> Self {
        Self {
            wave,
            at: 0.0,
            length,
            from,
            to,
            attack: 0.003,
            decay: 1.5,
            volume: 1.0,
            vibrato: (0.0, 0.0),
            lowpass: 0.0,
            tremolo: (0.0, 0.0),
        }
    }

    /// A steady pitch.
    pub const fn note(wave: Wave, freq: f32, length: f32) -> Self {
        Self::new(wave, freq, freq, length)
    }

    pub const fn at(mut self, at: f32) -> Self {
        self.at = at;
        self
    }

    pub const fn volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    pub const fn attack(mut self, attack: f32) -> Self {
        self.attack = attack;
        self
    }

    pub const fn decay(mut self, decay: f32) -> Self {
        self.decay = decay;
        self
    }

    pub const fn vibrato(mut self, depth: f32, rate: f32) -> Self {
        self.vibrato = (depth, rate);
        self
    }

    pub const fn lowpass(mut self, cutoff: f32) -> Self {
        self.lowpass = cutoff;
        self
    }

    pub const fn tremolo(mut self, depth: f32, rate: f32) -> Self {
        self.tremolo = (depth, rate);
        self
    }

    fn end(&self) -> f32 {
        self.at + self.length
    }

    /// Adds this tone into `out`; `seed` makes its noise its own.
    fn render(&self, out: &mut [f32], seed: u32) {
        let rate = SAMPLE_RATE as f32;
        let start = (self.at * rate) as usize;
        let samples = (self.length * rate) as usize;
        let mut phase = 0.0f32;
        let mut noise = Noise(seed.wrapping_mul(0x9E37_79B9) | 1);
        let mut held = noise.next();
        let mut filtered = 0.0f32;
        let alpha = if self.lowpass > 0.0 {
            let rc = 1.0 / (TAU * self.lowpass);
            (1.0 / rate) / (rc + 1.0 / rate)
        } else {
            1.0
        };
        for i in 0..samples.min(out.len().saturating_sub(start)) {
            let t = i as f32 / rate;
            let progress = i as f32 / samples as f32;
            let mut freq = self.from * (self.to / self.from).powf(progress);
            if self.vibrato.0 > 0.0 {
                freq *= 1.0 + self.vibrato.0 * (TAU * self.vibrato.1 * t).sin();
            }
            let last = phase;
            phase = (phase + freq / rate).fract();
            if self.wave == Wave::Noise && phase < last {
                held = noise.next();
            }
            let sample = match self.wave {
                Wave::Sine => (TAU * phase).sin(),
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Saw => 2.0 * phase - 1.0,
                Wave::Pulse(duty) => {
                    if phase < duty {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Noise => held,
            };
            filtered += alpha * (sample - filtered);
            let fade_in = if self.attack > 0.0 {
                (t / self.attack).min(1.0)
            } else {
                1.0
            };
            let fade_out = if self.decay > 0.0 {
                (1.0 - progress).powf(self.decay)
            } else {
                1.0
            };
            let wobble = 1.0 - self.tremolo.0 * 0.5 * (1.0 - (TAU * self.tremolo.1 * t).cos());
            out[start + i] += filtered * fade_in * fade_out * wobble * self.volume;
        }
    }
}

/// xorshift32: deterministic noise, so a sound is identical on every run.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Renders `tones` into mono samples peaking at `peak`, with a soft limiter and a short fade
/// at the end so nothing clicks.
pub fn render(tones: &[Tone], peak: f32) -> Vec<f32> {
    let length = tones.iter().map(Tone::end).fold(0.0, f32::max) + 0.01;
    let mut out = vec![0.0; (length * SAMPLE_RATE as f32) as usize];
    for (i, tone) in tones.iter().enumerate() {
        tone.render(&mut out, i as u32 + 1);
    }
    let loudest = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if loudest > 0.0 {
        for s in &mut out {
            *s = (*s / loudest * 1.2).tanh() / 1.2f32.tanh() * peak;
        }
    }
    let tail = (0.005 * SAMPLE_RATE as f32) as usize;
    let n = out.len();
    for (i, s) in out[n.saturating_sub(tail)..].iter_mut().enumerate() {
        *s *= 1.0 - i as f32 / tail as f32;
    }
    out
}

/// 16-bit mono WAV bytes, for `--dump-sfx`.
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_is_deterministic_and_peaks_where_asked() {
        let tones = [
            Tone::new(Wave::Noise, 2000.0, 200.0, 0.3).lowpass(3000.0),
            Tone::note(Wave::Pulse(0.25), 440.0, 0.2).at(0.1),
        ];
        let a = render(&tones, 0.8);
        assert_eq!(a, render(&tones, 0.8));
        let peak = a.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.8).abs() < 0.01, "peak {peak}");
        assert!(a.iter().all(|s| s.is_finite()));
        // Both tones end at 0.3 s, plus the 10 ms tail.
        assert_eq!(a.len(), ((0.3 + 0.01) * SAMPLE_RATE as f32) as usize);
    }

    #[test]
    fn a_glide_ends_at_its_target_pitch() {
        // Count zero crossings in the last 50 ms of a 1 s glide from 200 to 800 Hz.
        let s = render(&[Tone::new(Wave::Sine, 200.0, 800.0, 1.0).decay(0.0)], 1.0);
        let window = &s[(0.94 * SAMPLE_RATE as f32) as usize..(0.99 * SAMPLE_RATE as f32) as usize];
        let crossings = window
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        let hz = crossings as f32 / 0.05;
        assert!((700.0..=820.0).contains(&hz), "{hz} Hz");
    }
}
