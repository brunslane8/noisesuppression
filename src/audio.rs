//! Windows audio plumbing: real mic -> Hush -> virtual cable, plus a speaker
//! "loopback" tap that lets the echo canceller know what is being played.

use crate::dsp::{frame_rms, Pipeline, FRAME};
use crate::settings::Settings;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SampleFormat, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RATE: u32 = 48_000;
/// Ring buffer size (about 0.4 s). If it fills up we drop audio rather than lag.
const RING: usize = 19_200;
/// Longest delay we tolerate in the output queue before dropping old audio.
const MAX_QUEUE: usize = 4_800;

pub const CABLE_IN: &str = "CABLE Input";
pub const CABLE_OUT: &str = "CABLE Output";

/// Live values the window reads.
#[derive(Default)]
pub struct Shared {
    pub in_level: AtomicU32,
    pub out_level: AtomicU32,
    pub stop: AtomicBool,
    pub status: Mutex<Status>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    #[default]
    Starting,
    Running,
    NoCable,
    Problem(String),
}

impl Shared {
    fn set(&self, s: Status) {
        *self.status.lock().unwrap() = s;
    }
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    pub fn levels(&self) -> (f32, f32) {
        (
            f32::from_bits(self.in_level.load(Ordering::Relaxed)),
            f32::from_bits(self.out_level.load(Ordering::Relaxed)),
        )
    }
}

pub struct Engine {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    pub fn start(settings: Arc<Mutex<Settings>>) -> Engine {
        let shared = Arc::new(Shared::default());
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name("hush-audio".into())
            .spawn(move || supervise(settings, sh))
            .ok();
        Engine { shared, thread }
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn name_of(d: &Device) -> String {
    d.description().map(|d| d.name().to_string()).unwrap_or_default()
}

pub fn is_cable(name: &str) -> bool {
    name.contains("CABLE")
}

/// Names of real microphones (the virtual cable is left out).
pub fn list_mics() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|it| it.map(|d| name_of(&d)).filter(|n| !n.is_empty() && !is_cable(n)).collect())
        .unwrap_or_default()
}

pub fn cable_installed() -> bool {
    let host = cpal::default_host();
    host.output_devices()
        .map(|mut it| it.any(|d| name_of(&d).contains(CABLE_IN)))
        .unwrap_or(false)
}

fn endpoint_id(d: &Device) -> Option<String> {
    d.id().ok().map(|i| i.id().to_string())
}

/// Endpoint id of the "CABLE Output" microphone, used to make it the default mic.
pub fn cable_output_id() -> Option<String> {
    let host = cpal::default_host();
    host.input_devices().ok()?.find(|d| name_of(d).contains(CABLE_OUT)).and_then(|d| endpoint_id(&d))
}

/// Endpoint id of the "CABLE Input" speaker.
pub fn cable_input_id() -> Option<String> {
    let host = cpal::default_host();
    host.output_devices().ok()?.find(|d| name_of(d).contains(CABLE_IN)).and_then(|d| endpoint_id(&d))
}

/// Endpoint id of the first real (non-cable) speaker or headphones.
pub fn real_output_id() -> Option<String> {
    let host = cpal::default_host();
    host.output_devices().ok()?.find(|d| !is_cable(&name_of(d))).and_then(|d| endpoint_id(&d))
}

fn pick_mic(want: &str, original_id: &str) -> Option<Device> {
    let host = cpal::default_host();
    if !want.is_empty() {
        if let Some(d) = host.input_devices().ok()?.find(|d| name_of(d) == want) {
            return Some(d);
        }
    }
    // The mic that was the Windows default before Hush took that spot.
    if !original_id.is_empty() {
        if let Some(d) = host.input_devices().ok()?.find(|d| endpoint_id(d).as_deref() == Some(original_id)) {
            return Some(d);
        }
    }
    // Windows default, unless the default is our own virtual cable (that would loop).
    if let Some(d) = host.default_input_device() {
        if !is_cable(&name_of(&d)) {
            return Some(d);
        }
    }
    host.input_devices().ok()?.find(|d| !is_cable(&name_of(d)))
}

fn supervise(settings: Arc<Mutex<Settings>>, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::SeqCst) {
        match run_once(&settings, &shared) {
            Ok(()) => break,
            Err(Status::NoCable) => shared.set(Status::NoCable),
            Err(other) => shared.set(other),
        }
        for _ in 0..20 {
            if shared.stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

type Ring = ringbuf::HeapProd<f32>;
type RingOut = ringbuf::HeapCons<f32>;

/// Converts device audio to mono 48 kHz and queues it.
struct MonoFeeder {
    prod: Ring,
    channels: usize,
    step: f64, // device samples consumed per output sample
    pos: f64,
    prev: f32,
}

impl MonoFeeder {
    fn new(prod: Ring, channels: u16, rate: u32) -> Self {
        Self { prod, channels: channels as usize, step: rate as f64 / RATE as f64, pos: 0.0, prev: 0.0 }
    }

    fn feed<T: Sample>(&mut self, data: &[T])
    where
        f32: FromSample<T>,
    {
        for frame in data.chunks(self.channels) {
            let cur: f32 = frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / self.channels as f32;
            if (self.step - 1.0).abs() < 1e-9 {
                let _ = self.prod.try_push(cur);
                continue;
            }
            // Linear resampling: emit output samples that fall between prev and cur.
            while self.pos < 1.0 {
                let v = self.prev + (cur - self.prev) * self.pos as f32;
                let _ = self.prod.try_push(v);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = cur;
        }
    }
}

/// Reads queued 48 kHz mono audio and writes it to an output device.
struct MonoPlayer {
    cons: RingOut,
    channels: usize,
    step: f64,
    pos: f64,
    prev: f32,
    cur: f32,
}

impl MonoPlayer {
    fn new(cons: RingOut, channels: u16, rate: u32) -> Self {
        Self { cons, channels: channels as usize, step: RATE as f64 / rate as f64, pos: 0.0, prev: 0.0, cur: 0.0 }
    }

    fn next(&mut self) -> f32 {
        if (self.step - 1.0).abs() < 1e-9 {
            return self.cons.try_pop().unwrap_or(0.0);
        }
        self.pos += self.step;
        while self.pos >= 1.0 {
            self.prev = self.cur;
            self.cur = self.cons.try_pop().unwrap_or(0.0);
            self.pos -= 1.0;
        }
        self.prev + (self.cur - self.prev) * self.pos as f32
    }

    fn fill<T: Sample + FromSample<f32>>(&mut self, out: &mut [T]) {
        for frame in out.chunks_mut(self.channels) {
            let v = T::from_sample(self.next());
            frame.fill(v);
        }
    }
}

/// Error callback that only asks for a reconnect when a device is actually gone.
/// cpal also reports harmless events here (glitches, rerouting, real-time priority refused),
/// and restarting on those made Hush reconnect over and over.
fn fatal_errors(err: Arc<AtomicBool>) -> impl FnMut(cpal::Error) + Send + 'static {
    move |e: cpal::Error| {
        use cpal::ErrorKind::*;
        if !matches!(e.kind(), Xrun | DeviceChanged | RealtimeDenied) {
            err.store(true, Ordering::SeqCst);
        }
    }
}

fn input_stream(dev: &Device, mut feeder: MonoFeeder, err: Arc<AtomicBool>) -> Result<Stream, String> {
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let sc: StreamConfig = cfg.into();
    let on_err = fatal_errors(err);
    macro_rules! build {
        ($t:ty) => {
            dev.build_input_stream(sc, move |d: &[$t], _| feeder.feed(d), on_err, None)
        };
    }
    let s = match cfg.sample_format() {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I32 => build!(i32),
        other => return Err(format!("unsupported audio format {other}")),
    };
    s.map_err(|e| e.to_string())
}

fn output_stream(dev: &Device, mut player: MonoPlayer, err: Arc<AtomicBool>) -> Result<Stream, String> {
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let sc: StreamConfig = cfg.into();
    let on_err = fatal_errors(err);
    macro_rules! build {
        ($t:ty) => {
            dev.build_output_stream(sc, move |d: &mut [$t], _| player.fill(d), on_err, None)
        };
    }
    let s = match cfg.sample_format() {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I32 => build!(i32),
        other => return Err(format!("unsupported audio format {other}")),
    };
    s.map_err(|e| e.to_string())
}

fn channels_rate_in(d: &Device) -> Option<(u16, u32)> {
    let c = d.default_input_config().ok()?;
    Some((c.channels(), c.sample_rate()))
}
fn channels_rate_out(d: &Device) -> Option<(u16, u32)> {
    let c = d.default_output_config().ok()?;
    Some((c.channels(), c.sample_rate()))
}

fn run_once(settings: &Arc<Mutex<Settings>>, shared: &Arc<Shared>) -> Result<(), Status> {
    let host = cpal::default_host();
    let problem = |m: &str| Status::Problem(m.to_string());

    let cable = host
        .output_devices()
        .map_err(|e| Status::Problem(e.to_string()))?
        .find(|d| name_of(d).contains(CABLE_IN))
        .ok_or(Status::NoCable)?;
    let (want, original) = {
        let s = settings.lock().unwrap();
        (s.mic_device.clone(), s.original_mic_id.clone())
    };
    let mic = pick_mic(&want, &original).ok_or_else(|| problem("No microphone found. Plug one in and Hush will pick it up."))?;
    let speakers = host.default_output_device();

    let err = Arc::new(AtomicBool::new(false));
    let (mic_c, mic_r) = channels_rate_in(&mic).ok_or_else(|| problem("Can't read the microphone settings."))?;
    let (cab_c, cab_r) = channels_rate_out(&cable).ok_or_else(|| problem("Can't read the virtual cable settings."))?;

    let (mic_p, mut mic_q) = HeapRb::<f32>::new(RING).split();
    let (mut out_p, out_q) = HeapRb::<f32>::new(RING).split();
    let (mut mon_p, mon_q) = HeapRb::<f32>::new(RING).split();
    let (spk_p, mut spk_q) = HeapRb::<f32>::new(RING).split();

    let mic_stream = input_stream(&mic, MonoFeeder::new(mic_p, mic_c, mic_r), err.clone())
        .map_err(|e| Status::Problem(format!("Can't open the microphone: {e}")))?;
    let cable_stream = output_stream(&cable, MonoPlayer::new(out_q, cab_c, cab_r), err.clone())
        .map_err(|e| Status::Problem(format!("Can't open the virtual cable: {e}")))?;

    // Optional streams: speaker tap for echo removal, and "hear myself".
    let mut _spk_stream = None;
    let mut _mon_stream = None;
    if let Some(sp) = speakers.as_ref().filter(|d| !is_cable(&name_of(d))) {
        if let Some((c, r)) = channels_rate_out(sp) {
            // Opening an output device as an input turns on WASAPI loopback.
            _spk_stream = input_stream_loopback(sp, MonoFeeder::new(spk_p, c, r), err.clone()).ok();
            _mon_stream = output_stream(sp, MonoPlayer::new(mon_q, c, r), err.clone()).ok();
        }
    }

    mic_stream.play().map_err(|e| Status::Problem(e.to_string()))?;
    cable_stream.play().map_err(|e| Status::Problem(e.to_string()))?;
    if let Some(s) = &_spk_stream {
        let _ = s.play();
    }
    if let Some(s) = &_mon_stream {
        let _ = s.play();
    }
    shared.set(Status::Running);

    let mut pipeline = Pipeline::new(&settings.lock().unwrap());
    let mut frame = [0.0f32; FRAME];
    let mut spk = [0.0f32; FRAME];
    let mut spk_rms = 0.0f32;
    let mut in_hold = 0.0f32;
    let mut out_hold = 0.0f32;

    while !shared.stop.load(Ordering::SeqCst) {
        if err.load(Ordering::SeqCst) {
            return Err(problem("The audio device changed. Reconnecting..."));
        }
        // Keep latency low if the mic runs a bit faster than the cable.
        while mic_q.occupied_len() > MAX_QUEUE + FRAME {
            let mut skip = [0.0f32; FRAME];
            mic_q.pop_slice(&mut skip);
        }
        if mic_q.occupied_len() < FRAME {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        mic_q.pop_slice(&mut frame);

        if spk_q.occupied_len() >= FRAME {
            while spk_q.occupied_len() > MAX_QUEUE + FRAME {
                spk_q.pop_slice(&mut spk);
            }
            spk_q.pop_slice(&mut spk);
            spk_rms = 0.8 * spk_rms + 0.2 * frame_rms(&spk);
        } else {
            spk = [0.0; FRAME];
            spk_rms *= 0.8;
        }
        pipeline.push_speaker_frame(&spk);

        let s = settings.lock().unwrap().clone();
        pipeline.process(&s, &mut frame, spk_rms);

        if out_p.vacant_len() >= FRAME {
            out_p.push_slice(&frame);
        }
        if s.hear_myself && mon_p.vacant_len() >= FRAME {
            mon_p.push_slice(&frame);
        }

        // Meters fall off slowly so they are easy to read.
        in_hold = pipeline.in_peak.max(in_hold * 0.9);
        out_hold = pipeline.out_peak.max(out_hold * 0.9);
        shared.in_level.store(in_hold.to_bits(), Ordering::Relaxed);
        shared.out_level.store(out_hold.to_bits(), Ordering::Relaxed);
    }
    Ok(())
}

fn input_stream_loopback(dev: &Device, feeder: MonoFeeder, err: Arc<AtomicBool>) -> Result<Stream, String> {
    // Same as a normal input stream: cpal's WASAPI host switches to loopback for output devices.
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let sc: StreamConfig = cfg.into();
    let mut feeder = feeder;
    let on_err = fatal_errors(err);
    macro_rules! build {
        ($t:ty) => {
            dev.build_input_stream(sc, move |d: &[$t], _| feeder.feed(d), on_err, None)
        };
    }
    let s = match cfg.sample_format() {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I32 => build!(i32),
        other => return Err(format!("unsupported audio format {other}")),
    };
    s.map_err(|e| e.to_string())
}
