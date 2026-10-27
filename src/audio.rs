//! Procedural 8-bit audio: square/triangle/noise voices rendered at startup.
//!
//! Everything is synthesized in code, so the game ships without any audio files.
//! Sound effects are pre-rendered into short buffers and mixed by rodio.

use rodio::buffer::SamplesBuffer;
use rodio::mixer::Mixer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::num::NonZero;

const RATE: u32 = 44_100;

/// A classic 80s console-style oscillator.
#[derive(Clone, Copy)]
enum Wave {
    /// Pulse wave with the given duty cycle (0.125/0.25/0.5 are the sweet spots).
    Pulse(f32),
    /// Stepped 4-bit triangle, like the NES triangle channel.
    Triangle,
    /// LFSR pseudo-noise, like the NES noise channel.
    Noise,
}

/// One-shot sound effects. `Brick` and `Paddle` get some variation in playback.
#[derive(Clone, Copy)]
pub enum Sfx {
    Launch,
    Paddle,
    Wall,
    Brick(usize),
    LifeLost,
    GameOver,
    LevelClear,
    Pause,
    Resume,
}

const BRICK_NOTES: [f32; 5] = [1046.50, 880.00, 783.99, 659.25, 523.25];

/// All effects are rendered once into memory and cloned cheaply for playback.
struct SoundBank {
    launch: SamplesBuffer,
    paddle: [SamplesBuffer; 2],
    wall: SamplesBuffer,
    brick: Vec<SamplesBuffer>,
    life_lost: SamplesBuffer,
    game_over: SamplesBuffer,
    level_clear: SamplesBuffer,
    pause: SamplesBuffer,
    resume: SamplesBuffer,
    music: SamplesBuffer,
}

impl SoundBank {
    fn new() -> Self {
        Self {
            launch: buffer(sfx_launch()),
            paddle: [buffer(sfx_paddle(0.0)), buffer(sfx_paddle(0.18))],
            wall: buffer(sfx_wall()),
            brick: (0..5)
                .flat_map(|row| [buffer(sfx_brick(row, true)), buffer(sfx_brick(row, false))])
                .collect(),
            life_lost: buffer(sfx_life_lost()),
            game_over: buffer(sfx_game_over()),
            level_clear: buffer(sfx_level_clear()),
            pause: buffer(sfx_pause(false)),
            resume: buffer(sfx_pause(true)),
            music: buffer(render_music()),
        }
    }

    fn source(&self, sfx: Sfx, bright_variant: bool) -> SamplesBuffer {
        match sfx {
            Sfx::Launch => self.launch.clone(),
            Sfx::Paddle => self.paddle[bright_variant as usize].clone(),
            Sfx::Wall => self.wall.clone(),
            Sfx::Brick(row) => self.brick[row.min(4) * 2 + bright_variant as usize].clone(),
            Sfx::LifeLost => self.life_lost.clone(),
            Sfx::GameOver => self.game_over.clone(),
            Sfx::LevelClear => self.level_clear.clone(),
            Sfx::Pause => self.pause.clone(),
            Sfx::Resume => self.resume.clone(),
        }
    }
}

/// Owns the output device and the music player. Dropping it stops all audio.
pub struct Audio {
    mixer: Mixer,
    music: Player,
    bank: SoundBank,
    muted: bool,
    music_wanted: bool,
    flip: bool,
    // Declared last so the device outlives the players that feed it.
    _sink: MixerDeviceSink,
}

impl Audio {
    /// Open the default output device. Returns `None` (and logs a warning) when
    /// no audio device is available, so the game still runs silently.
    pub fn new() -> Option<Self> {
        let mut sink = match DeviceSinkBuilder::open_default_sink() {
            Ok(sink) => sink,
            Err(err) => {
                eprintln!("audio disabled: {err}");
                return None;
            }
        };
        sink.log_on_drop(false);
        let mixer = sink.mixer().clone();
        let bank = SoundBank::new();
        let music = Player::connect_new(&mixer);
        music.set_volume(0.45);
        music.append(bank.music.clone().repeat_infinite());
        music.pause();
        Some(Self {
            mixer,
            music,
            bank,
            muted: false,
            music_wanted: false,
            flip: false,
            _sink: sink,
        })
    }

    pub fn play(&mut self, sfx: Sfx) {
        if self.muted {
            return;
        }
        self.flip = !self.flip;
        self.mixer.add(self.bank.source(sfx, self.flip));
    }

    /// Pause/resume the background loop. Safe to call repeatedly.
    pub fn set_music(&mut self, playing: bool) {
        self.music_wanted = playing;
        if playing && !self.muted {
            self.music.play();
        } else {
            self.music.pause();
        }
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if self.muted {
            self.music.pause();
        } else {
            if self.music_wanted {
                self.music.play();
            }
            self.play(Sfx::Resume);
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }
}

fn buffer(samples: Vec<f32>) -> SamplesBuffer {
    SamplesBuffer::new(
        NonZero::new(1).unwrap(),
        NonZero::new(RATE).unwrap(),
        samples,
    )
}

// ---------------------------------------------------------------------------
// Synthesis
// ---------------------------------------------------------------------------

/// Mix one note/sweep into `out`, starting at `at` seconds.
///
/// `f0` and `f1` are the start/end frequency (a glide when they differ); for
/// `Wave::Noise` they set the LFSR clock rate. `decay` is the pluck exponent:
/// 0 is a flat sustain, higher values fade faster.
#[allow(clippy::too_many_arguments)]
fn add_note(
    out: &mut Vec<f32>,
    at: f32,
    f0: f32,
    f1: f32,
    dur: f32,
    vol: f32,
    wave: Wave,
    decay: f32,
) {
    let start = (at.max(0.0) * RATE as f32) as usize;
    let len = (dur.max(0.0) * RATE as f32) as usize;
    if len == 0 {
        return;
    }
    if out.len() < start + len {
        out.resize(start + len, 0.0);
    }

    let mut phase = 0.0f32;
    let mut noise_phase = 0.0f32;
    // Deterministic per-note noise seed.
    let mut lfsr: u16 = (0x7fff ^ (start as u16).wrapping_mul(40_503)) | 1;
    let attack_len = (0.0015 * RATE as f32).max(1.0);
    let release_len = (0.006 * RATE as f32).max(1.0);

    for i in 0..len {
        let t = i as f32 / len as f32;
        let freq = if f0 > 0.0 && f1 > 0.0 {
            f0 * (f1 / f0).powf(t)
        } else {
            f0.max(0.0)
        };

        let raw = match wave {
            Wave::Pulse(duty) => {
                if phase < duty {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Triangle => {
                let v = 2.0 * (2.0 * phase - 1.0).abs() - 1.0;
                (v * 7.0).round() / 7.0 // 4-bit steps
            }
            Wave::Noise => {
                noise_phase += freq / RATE as f32;
                while noise_phase >= 1.0 {
                    noise_phase -= 1.0;
                    let bit = (lfsr ^ (lfsr >> 1)) & 1;
                    lfsr = (lfsr >> 1) | (bit << 14);
                }
                if lfsr & 1 == 0 { 1.0 } else { -1.0 }
            }
        };

        let attack = (i as f32 / attack_len).min(1.0);
        let release = ((len - i) as f32 / release_len).min(1.0);
        let envelope = attack
            * release
            * if decay > 0.0 {
                (1.0 - t).powf(decay)
            } else {
                1.0
            };

        out[start + i] += raw * envelope * vol;

        phase += freq / RATE as f32;
        if phase >= 1.0 {
            phase -= 1.0;
        }
    }
}

/// Soft-clip, scale down only if needed, and quantize for that crunchy 8-bit bite.
fn finish(out: &mut [f32]) {
    let mut peak = 0.0f32;
    for sample in out.iter_mut() {
        if !sample.is_finite() {
            *sample = 0.0;
        }
        peak = peak.max(sample.abs());
    }
    if peak > 0.9 {
        let gain = 0.9 / peak;
        for sample in out.iter_mut() {
            *sample *= gain;
        }
    }
    for sample in out.iter_mut() {
        *sample = (*sample * 31.0).round() / 31.0; // 5-bit amplitude
    }
}

// ---------------------------------------------------------------------------
// Sound design
// ---------------------------------------------------------------------------

fn sfx_paddle(brightness: f32) -> Vec<f32> {
    let mut out = Vec::new();
    add_note(
        &mut out,
        0.0,
        520.0 + brightness * 120.0,
        760.0 + brightness * 180.0,
        0.055,
        0.42,
        Wave::Pulse(0.5),
        1.0,
    );
    add_note(&mut out, 0.0, 165.0, 105.0, 0.06, 0.22, Wave::Triangle, 2.0);
    finish(&mut out);
    out
}

fn sfx_wall() -> Vec<f32> {
    let mut out = Vec::new();
    add_note(
        &mut out,
        0.0,
        240.0,
        170.0,
        0.024,
        0.26,
        Wave::Pulse(0.125),
        1.5,
    );
    finish(&mut out);
    out
}

fn sfx_brick(row: usize, up: bool) -> Vec<f32> {
    let base = BRICK_NOTES[row.min(4)];
    let (first, second) = if up {
        (base, base * 1.5)
    } else {
        (base * 1.5, base)
    };
    let mut out = Vec::new();
    add_note(
        &mut out,
        0.0,
        first,
        first,
        0.034,
        0.34,
        Wave::Pulse(0.25),
        0.7,
    );
    add_note(
        &mut out,
        0.024,
        second,
        second * 0.995,
        0.062,
        0.30,
        Wave::Pulse(0.25),
        1.5,
    );
    add_note(&mut out, 0.0, 9000.0, 4500.0, 0.008, 0.10, Wave::Noise, 2.0);
    finish(&mut out);
    out
}

fn sfx_launch() -> Vec<f32> {
    let mut out = Vec::new();
    add_note(
        &mut out,
        0.0,
        280.0,
        1250.0,
        0.20,
        0.34,
        Wave::Pulse(0.125),
        0.6,
    );
    add_note(
        &mut out,
        0.02,
        140.0,
        625.0,
        0.18,
        0.18,
        Wave::Triangle,
        0.8,
    );
    add_note(&mut out, 0.0, 7000.0, 2500.0, 0.18, 0.03, Wave::Noise, 1.2);
    finish(&mut out);
    out
}

fn sfx_life_lost() -> Vec<f32> {
    let mut out = Vec::new();
    for (i, freq) in [392.0, 329.63, 261.63, 196.0].into_iter().enumerate() {
        add_note(
            &mut out,
            i as f32 * 0.09,
            freq,
            freq * 0.99,
            0.105,
            0.34,
            Wave::Pulse(0.5),
            0.8,
        );
    }
    add_note(&mut out, 0.36, 196.0, 98.0, 0.34, 0.26, Wave::Triangle, 0.5);
    finish(&mut out);
    out
}

fn sfx_game_over() -> Vec<f32> {
    let mut out = Vec::new();
    let notes = [440.0, 349.23, 293.66, 220.0, 146.83];
    for (i, freq) in notes.into_iter().enumerate() {
        let at = i as f32 * 0.15;
        add_note(
            &mut out,
            at,
            freq,
            freq * 0.985,
            0.18,
            0.34,
            Wave::Pulse(0.5),
            0.5,
        );
        add_note(
            &mut out,
            at,
            freq * 0.5,
            freq * 0.49,
            0.18,
            0.18,
            Wave::Triangle,
            0.4,
        );
    }
    add_note(
        &mut out,
        0.75,
        110.0,
        55.0,
        0.65,
        0.28,
        Wave::Pulse(0.25),
        0.25,
    );
    finish(&mut out);
    out
}

fn sfx_level_clear() -> Vec<f32> {
    let mut out = Vec::new();
    for (i, freq) in [523.25, 659.25, 783.99, 1046.50].into_iter().enumerate() {
        add_note(
            &mut out,
            i as f32 * 0.068,
            freq,
            freq,
            0.075,
            0.28,
            Wave::Pulse(0.25),
            0.5,
        );
    }
    for freq in [1046.50, 1318.51, 1567.98] {
        add_note(
            &mut out,
            0.28,
            freq,
            freq * 0.998,
            0.46,
            0.17,
            Wave::Pulse(0.25),
            0.9,
        );
    }
    add_note(
        &mut out,
        0.28,
        130.81,
        130.81,
        0.46,
        0.26,
        Wave::Triangle,
        0.6,
    );
    add_note(
        &mut out,
        0.28,
        8000.0,
        3500.0,
        0.30,
        0.035,
        Wave::Noise,
        1.8,
    );
    finish(&mut out);
    out
}

fn sfx_pause(resume: bool) -> Vec<f32> {
    let (a, b) = if resume {
        (392.0, 587.33)
    } else {
        (587.33, 392.0)
    };
    let mut out = Vec::new();
    add_note(&mut out, 0.0, a, a, 0.045, 0.28, Wave::Pulse(0.5), 0.8);
    add_note(&mut out, 0.06, b, b, 0.055, 0.28, Wave::Pulse(0.5), 0.8);
    finish(&mut out);
    out
}

/// A 1980s arcade loop: Am–F–C–G, twice, with an E-major lift at the end.
/// Rendered once and played through `repeat_infinite()`.
fn render_music() -> Vec<f32> {
    const BPM: f32 = 138.0;
    const STEP: f32 = 60.0 / BPM / 4.0; // 16th note
    const BARS: usize = 8;
    let pattern = [0usize, 1, 2, 1, 0, 2, 1, 2, 0, 1, 2, 1, 0, 2, 1, 2];
    let chords: [[f32; 3]; BARS] = [
        [220.00, 261.63, 329.63], // Am
        [174.61, 220.00, 261.63], // F
        [261.63, 329.63, 392.00], // C
        [196.00, 246.94, 293.66], // G
        [220.00, 261.63, 329.63], // Am
        [174.61, 220.00, 261.63], // F
        [196.00, 246.94, 293.66], // G
        [164.81, 207.65, 246.94], // E
    ];
    let roots = [110.00, 87.31, 130.81, 98.00, 110.00, 87.31, 98.00, 82.41];

    let total = BARS as f32 * 16.0 * STEP;
    let mut out = vec![0.0; (total * RATE as f32) as usize + 1];
    let mut lead = vec![0.0; out.len()];

    for bar in 0..BARS {
        for step in 0..16 {
            let at = (bar * 16 + step) as f32 * STEP;
            let chord = chords[bar];

            // Bouncing arpeggio lead.
            let octave = if step % 8 < 4 { 2.0 } else { 4.0 };
            let freq = chord[pattern[step]] * octave;
            add_note(
                &mut lead,
                at,
                freq,
                freq,
                STEP * 0.82,
                0.085,
                Wave::Pulse(0.25),
                1.4,
            );

            // Triangle bass with a syncopated pickup.
            if step % 4 == 0 || step % 8 == 6 {
                let bass = roots[bar] * if step % 8 == 4 { 2.0 } else { 1.0 };
                add_note(
                    &mut out,
                    at,
                    bass,
                    bass,
                    STEP * 3.2,
                    0.20,
                    Wave::Triangle,
                    0.35,
                );
            }

            // Noise hats on the off-beats.
            if step % 2 == 1 {
                add_note(&mut out, at, 9000.0, 7000.0, 0.022, 0.028, Wave::Noise, 3.0);
            }

            // Snare on beats two and four.
            if step == 4 || step == 12 {
                add_note(&mut out, at, 2200.0, 900.0, 0.10, 0.06, Wave::Noise, 2.0);
                add_note(&mut out, at, 190.0, 120.0, 0.08, 0.10, Wave::Triangle, 2.0);
            }
        }
    }

    // Slapback echo on the lead.
    let delay = (STEP * 0.75 * RATE as f32) as usize;
    for i in 0..lead.len() {
        let value = lead[i];
        if i + delay < out.len() {
            out[i + delay] += value * 0.22;
        }
        out[i] += value;
    }

    finish(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |p, s| p.max(s.abs()))
    }

    #[test]
    fn all_effects_are_audible_and_bounded() {
        let bank = SoundBank::new();
        let effects: Vec<(&str, &SamplesBuffer)> = vec![
            ("launch", &bank.launch),
            ("wall", &bank.wall),
            ("life", &bank.life_lost),
            ("over", &bank.game_over),
            ("clear", &bank.level_clear),
            ("pause", &bank.pause),
            ("resume", &bank.resume),
            ("music", &bank.music),
        ];
        for (name, effect) in effects {
            let samples: Vec<f32> = effect.clone().collect();
            assert!(!samples.is_empty(), "{name} is empty");
            let p = peak(&samples);
            assert!(p > 0.01, "{name} is silent");
            assert!(p <= 0.91, "{name} is too hot: {p}");
            assert!(samples.iter().all(|s| s.is_finite()), "{name} has NaN");
        }
        for row in 0..5 {
            for up in [false, true] {
                let samples: Vec<f32> = buffer(sfx_brick(row, up)).collect();
                let p = peak(&samples);
                assert!(p > 0.01 && p <= 0.91, "brick {row} peak {p}");
            }
        }
    }

    #[test]
    fn music_is_a_loopable_length() {
        let bank = SoundBank::new();
        let seconds = bank.music.clone().count() as f32 / RATE as f32;
        assert!((7.0..16.0).contains(&seconds), "music is {seconds}s");
    }

    #[test]
    fn sweeps_move_in_the_right_direction() {
        let mut up = Vec::new();
        add_note(&mut up, 0.0, 200.0, 800.0, 0.05, 0.5, Wave::Pulse(0.5), 0.0);
        let mut down = Vec::new();
        add_note(
            &mut down,
            0.0,
            800.0,
            200.0,
            0.05,
            0.5,
            Wave::Pulse(0.5),
            0.0,
        );
        // Count zero crossings as a cheap proxy for average pitch.
        let crossings = |samples: &[f32]| {
            samples
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        let quarter = up.len() / 4;
        assert!(crossings(&up[3 * quarter..]) > crossings(&up[..quarter]));
        assert!(crossings(&down[3 * quarter..]) < crossings(&down[..quarter]));
    }
}
