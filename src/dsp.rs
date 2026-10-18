//! The sound-cleaning chain. Platform independent, works on 10 ms frames of
//! mono 48 kHz audio (480 samples).

use crate::settings::Settings;
use nnnoiseless::DenoiseState;
use sonora::config::{EchoCanceller, GainController2, NoiseSuppression, NoiseSuppressionLevel};
use sonora::{AudioProcessing, Config, StreamConfig};

pub const FRAME: usize = 480;
const RATE: usize = 48_000;
const I16: f32 = 32768.0;

/// What changes sonora's configuration; compared so we only re-apply on change.
#[derive(PartialEq, Clone, Copy)]
struct ApmKey {
    hum: u8,
    echo_on: bool,
    agc: bool,
}

pub struct Pipeline {
    apm: AudioProcessing,
    key: ApmKey,
    rnn: Box<DenoiseState<'static>>,
    gate_gain: f32,
    gate_hold: u32,
    duck_gain: f32,
    pub in_peak: f32,
    pub out_peak: f32,
}

fn apm_config(key: ApmKey) -> Config {
    let level = match key.hum {
        1 => Some(NoiseSuppressionLevel::Low),
        2 => Some(NoiseSuppressionLevel::Moderate),
        3 => Some(NoiseSuppressionLevel::High),
        4 => Some(NoiseSuppressionLevel::VeryHigh),
        _ => None,
    };
    Config {
        echo_canceller: key.echo_on.then(EchoCanceller::default),
        noise_suppression: level.map(|level| NoiseSuppression {
            level,
            ..Default::default()
        }),
        gain_controller2: key.agc.then(GainController2::default),
        ..Default::default()
    }
}

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn rms(buf: &[f32]) -> f32 {
    (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
}

impl Pipeline {
    pub fn new(s: &Settings) -> Self {
        let key = Self::key(s);
        let apm = AudioProcessing::builder()
            .config(apm_config(key))
            .capture_config(StreamConfig::new(RATE as u32, 1))
            .render_config(StreamConfig::new(RATE as u32, 1))
            .build();
        Self {
            apm,
            key,
            rnn: DenoiseState::new(),
            gate_gain: 1.0,
            gate_hold: 0,
            duck_gain: 1.0,
            in_peak: 0.0,
            out_peak: 0.0,
        }
    }

    fn key(s: &Settings) -> ApmKey {
        ApmKey {
            hum: s.hum_level.min(4),
            echo_on: s.echo_removal > 0.5,
            agc: s.even_volume,
        }
    }

    /// Feed what is playing on the speakers so echo can be recognised.
    pub fn push_speaker_frame(&mut self, speaker: &[f32; FRAME]) {
        let mut out = [0.0f32; FRAME];
        let _ = self.apm.process_render_f32(&[&speaker[..]], &mut [&mut out[..]]);
    }

    /// Clean one frame of microphone audio in place.
    pub fn process(&mut self, s: &Settings, frame: &mut [f32; FRAME], speaker_rms: f32) {
        let key = Self::key(s);
        if key != self.key {
            self.key = key;
            self.apm.apply_config(apm_config(key));
        }

        let gain = db_to_lin(s.mic_gain_db.clamp(-20.0, 20.0));
        for x in frame.iter_mut() {
            *x *= gain;
        }
        self.in_peak = peak(frame);

        // Echo removal + steady hum removal + volume levelling.
        let mut stage1 = [0.0f32; FRAME];
        if self.apm.process_capture_f32(&[&frame[..]], &mut [&mut stage1[..]]).is_err() {
            stage1 = *frame;
        }

        // AI noise removal (keyboards, dogs, traffic...), blended by strength.
        let mut scaled = [0.0f32; FRAME];
        for (d, x) in scaled.iter_mut().zip(stage1.iter()) {
            *d = x * I16;
        }
        let mut cleaned = [0.0f32; FRAME];
        let voice = self.rnn.process_frame(&mut cleaned, &scaled);
        let wet = (s.noise_removal / 100.0).clamp(0.0, 1.0);
        for i in 0..FRAME {
            frame[i] = stage1[i] * (1.0 - wet) + (cleaned[i] / I16) * wet;
        }

        // Extra echo ducking: while the speakers are loud and it doesn't sound like
        // you're talking, turn the mic down.
        let echo = (s.echo_removal / 100.0).clamp(0.0, 1.0);
        let duck_target = if echo > 0.0 && speaker_rms > 0.01 && voice < 0.6 {
            1.0 - 0.85 * echo
        } else {
            1.0
        };
        let rate = if duck_target < self.duck_gain { 0.5 } else { 0.05 };
        self.duck_gain += (duck_target - self.duck_gain) * rate;

        // Silence gate between words, with a short hold so word endings survive.
        let strength = (s.silence_gate / 100.0).clamp(0.0, 1.0);
        let gate_target = if strength <= 0.0 {
            1.0
        } else {
            let threshold = 0.15 + 0.35 * strength;
            if voice > threshold {
                self.gate_hold = 25; // 250 ms
            } else {
                self.gate_hold = self.gate_hold.saturating_sub(1);
            }
            if self.gate_hold > 0 {
                1.0
            } else {
                db_to_lin(-(15.0 + 45.0 * strength))
            }
        };
        let rate = if gate_target > self.gate_gain { 0.4 } else { 0.08 };
        self.gate_gain += (gate_target - self.gate_gain) * rate;

        let g = self.duck_gain * self.gate_gain;
        for x in frame.iter_mut() {
            *x = soft_limit(*x * g);
        }
        self.out_peak = peak(frame);
    }
}

pub fn frame_rms(buf: &[f32]) -> f32 {
    rms(buf)
}

/// Gentle clipper so loud peaks never crackle.
fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        x.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u32);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
            (self.0 >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        }
    }

    fn run(s: &Settings, noise: f32, tone: f32, frames: usize) -> f32 {
        let mut p = Pipeline::new(s);
        let mut rng = Rng(7);
        let mut t = 0usize;
        let mut energy = 0.0;
        let mut n = 0;
        for f in 0..frames {
            let mut frame = [0.0f32; FRAME];
            for x in frame.iter_mut() {
                let v = tone * (2.0 * std::f32::consts::PI * 220.0 * t as f32 / 48000.0).sin();
                *x = v + noise * rng.next();
                t += 1;
            }
            p.process(s, &mut frame, 0.0);
            if f >= frames / 2 {
                energy += rms(&frame);
                n += 1;
            }
        }
        energy / n as f32
    }

    #[test]
    fn steady_noise_is_removed() {
        let s = Settings::default();
        let out = run(&s, 0.1, 0.0, 300);
        assert!(out < 0.02, "noise should drop a lot, got {out}");
    }

    #[test]
    fn everything_off_passes_audio_through() {
        let mut s = Settings::default();
        s.noise_removal = 0.0;
        s.hum_level = 0;
        s.echo_removal = 0.0;
        s.silence_gate = 0.0;
        s.even_volume = false;
        let out = run(&s, 0.0, 0.3, 200);
        assert!(out > 0.15, "tone should survive, got {out}");
    }

    #[test]
    fn gate_mutes_silence() {
        let mut s = Settings::default();
        s.silence_gate = 100.0;
        let out = run(&s, 0.005, 0.0, 300);
        assert!(out < 0.002, "got {out}");
    }

    #[test]
    fn gain_is_applied() {
        let mut s = Settings::default();
        s.noise_removal = 0.0;
        s.hum_level = 0;
        s.echo_removal = 0.0;
        s.silence_gate = 0.0;
        s.even_volume = false;
        let quiet = run(&s, 0.0, 0.05, 100);
        s.mic_gain_db = 12.0;
        let loud = run(&s, 0.0, 0.05, 100);
        assert!(loud > quiet * 3.0, "{quiet} -> {loud}");
    }
}
