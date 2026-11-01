//! Sound haptics: TRUEFORCE-style wheel vibration made from what the PC is playing.
//!
//! WASAPI loopback (via cpal) captures the speaker output. The audio callback band-passes
//! it to the chosen low range and tracks two things: a loudness envelope (auto-gained, so
//! speaker volume doesn't matter) and the dominant frequency (zero crossings), so engine
//! revs feel faster as they rise. A control thread turns that into a sine vibration on the
//! wheel, sending updates to the device thread only when they change noticeably.

use super::Cmd;
use crate::config::HapticBand;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Current haptic level 0..1 (after strength), for the UI meter.
static LEVEL: AtomicU32 = AtomicU32::new(0);

pub fn level() -> f32 {
    f32::from_bits(LEVEL.load(Ordering::Relaxed))
}

const TICK: Duration = Duration::from_millis(10);
/// Re-send a running vibration this often even if unchanged (keeps it alive after resets).
const KEEPALIVE: Duration = Duration::from_millis(500);
/// Below this normalised loudness the wheel stays still.
const GATE: f32 = 0.04;
const MAX_MAG: f32 = 0x7FFF as f32;

/// What the audio callback publishes for the control thread.
#[derive(Default)]
struct Analysis {
    /// Loudness 0..1, normalised to the recent peak.
    env: AtomicU32,
    /// Dominant frequency in Hz.
    freq: AtomicU32,
}

pub(super) struct Haptics {
    stop: Arc<AtomicBool>,
    strength: Arc<AtomicU8>,
    band: HapticBand,
    thread: Option<JoinHandle<()>>,
}

impl Haptics {
    pub fn start(band: HapticBand, strength: u8, mgr: Sender<Cmd>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let strength = Arc::new(AtomicU8::new(strength));
        let (s, st) = (stop.clone(), strength.clone());
        let thread = std::thread::Builder::new()
            .name("gclone-haptics".into())
            .spawn(move || run(band, s, st, mgr))
            .ok();
        Self { stop, strength, band, thread }
    }

    pub fn band(&self) -> HapticBand {
        self.band
    }

    pub fn set_strength(&self, percent: u8) {
        self.strength.store(percent.min(100), Ordering::Relaxed);
    }
}

impl Drop for Haptics {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(band: HapticBand, stop: Arc<AtomicBool>, strength: Arc<AtomicU8>, mgr: Sender<Cmd>) {
    let analysis = Arc::new(Analysis::default());
    // The stream must live on this thread; dropping it stops the capture.
    let _stream = match open_stream(band, analysis.clone()) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("sound haptics: cannot capture audio: {e}");
            return;
        }
    };
    log::info!("sound haptics on ({band:?})");

    let mut sent = (0u16, 0u16);
    let mut last_send = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(TICK);
        let env = f32::from_bits(analysis.env.load(Ordering::Relaxed));
        let freq = f32::from_bits(analysis.freq.load(Ordering::Relaxed));
        let gain = strength.load(Ordering::Relaxed) as f32 / 100.0;

        let level = if env < GATE { 0.0 } else { ((env - GATE) / (1.0 - GATE)).clamp(0.0, 1.0) * gain };
        LEVEL.store(level.to_bits(), Ordering::Relaxed);
        // Slight curve so quiet rumble is still felt but loud hits don't saturate early.
        let mag = (level.powf(0.8) * MAX_MAG) as u16;
        let period = (1000.0 / freq.clamp(10.0, 100.0)).round() as u16;
        let mag = if mag < 300 { 0 } else { mag };

        let changed = mag.abs_diff(sent.0) > (MAX_MAG / 40.0) as u16
            || (mag > 0 && period.abs_diff(sent.1) >= 2)
            || (mag == 0) != (sent.0 == 0);
        if changed || (mag > 0 && last_send.elapsed() >= KEEPALIVE) {
            let _ = mgr.send(Cmd::Haptic { magnitude: mag, period_ms: period });
            sent = (mag, period);
            last_send = Instant::now();
        }
    }
    LEVEL.store(0, Ordering::Relaxed);
    let _ = mgr.send(Cmd::Haptic { magnitude: 0, period_ms: 0 });
    log::info!("sound haptics off");
}

fn open_stream(band: HapticBand, analysis: Arc<Analysis>) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no audio output device")?;
    let supported = device.default_output_config()?;
    let config: cpal::StreamConfig = supported.config();
    let channels = config.channels as usize;
    let mut dsp = Dsp::new(band, config.sample_rate.0 as f32);
    let err = |e| log::warn!("sound haptics audio error: {e}");
    // On WASAPI, an input stream on an *output* device is a loopback capture.
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |d: &[f32], _: &cpal::InputCallbackInfo| dsp.process(d.iter().copied(), channels, &analysis),
            err,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |d: &[i16], _: &cpal::InputCallbackInfo| {
                dsp.process(d.iter().map(|s| *s as f32 / 32768.0), channels, &analysis)
            },
            err,
            None,
        )?,
        other => return Err(format!("unsupported sample format {other:?}").into()),
    };
    stream.play()?;
    Ok(stream)
}

/// Band-pass + envelope + zero-crossing pitch, all per sample, allocation-free.
struct Dsp {
    hp_a: f32,
    lp_b: f32,
    hp_x: f32,
    hp_y: f32,
    lp1: f32,
    lp2: f32,
    /// Envelope of the low band.
    env: f32,
    /// Envelope and recent peak of the *whole* signal: the low band is measured against
    /// overall loudness, so voices or music treble don't turn into vibration.
    full_env: f32,
    full_peak: f32,
    attack: f32,
    release: f32,
    peak_decay: f32,
    positive: bool,
    crossings: u32,
    count: u32,
    /// Samples per publish window (10 ms).
    window: u32,
    rate: f32,
    freq: f32,
}

impl Dsp {
    fn new(band: HapticBand, rate: f32) -> Self {
        let (lo, hi) = band.range_hz();
        let dt = 1.0 / rate;
        let rc_hp = 1.0 / (2.0 * std::f32::consts::PI * lo);
        let rc_lp = 1.0 / (2.0 * std::f32::consts::PI * hi);
        let coef = |seconds: f32| 1.0 - (-1.0 / (seconds * rate)).exp();
        Self {
            hp_a: rc_hp / (rc_hp + dt),
            lp_b: dt / (rc_lp + dt),
            hp_x: 0.0,
            hp_y: 0.0,
            lp1: 0.0,
            lp2: 0.0,
            env: 0.0,
            full_env: 0.0,
            full_peak: 0.0,
            attack: coef(0.005),
            release: coef(0.08),
            // Peak memory halves every ~4 s, so the gain adapts to volume changes.
            peak_decay: (-std::f32::consts::LN_2 / (4.0 * rate)).exp(),
            positive: false,
            crossings: 0,
            count: 0,
            window: (rate / 100.0) as u32,
            rate,
            freq: 40.0,
        }
    }

    fn process(&mut self, samples: impl Iterator<Item = f32>, channels: usize, out: &Analysis) {
        let mut mono = 0.0;
        for (i, s) in samples.enumerate() {
            mono += s;
            if (i + 1) % channels != 0 {
                continue;
            }
            let x = mono / channels as f32;
            mono = 0.0;

            // High-pass (removes DC / sub-rumble), then two low-pass stages.
            self.hp_y = self.hp_a * (self.hp_y + x - self.hp_x);
            self.hp_x = x;
            self.lp1 += self.lp_b * (self.hp_y - self.lp1);
            self.lp2 += self.lp_b * (self.lp1 - self.lp2);
            let y = self.lp2;

            let a = y.abs();
            let k = if a > self.env { self.attack } else { self.release };
            self.env += k * (a - self.env);
            let fa = x.abs();
            let k = if fa > self.full_env { self.attack } else { self.release };
            self.full_env += k * (fa - self.full_env);
            self.full_peak = self.full_env.max(self.full_peak * self.peak_decay);

            let pos = y > 0.0;
            if pos != self.positive && a > self.env * 0.2 {
                self.crossings += 1;
            }
            self.positive = pos;

            self.count += 1;
            if self.count >= self.window {
                let seconds = self.count as f32 / self.rate;
                let f = self.crossings as f32 / 2.0 / seconds;
                if self.crossings >= 2 {
                    self.freq += 0.3 * (f - self.freq);
                }
                // Low-band loudness relative to the recent overall loudness (half of it counts
                // as full scale), with a ~-50 dBFS floor so silence isn't amplified into noise.
                let norm = self.env / (self.full_peak * 0.5).max(0.003);
                out.env.store(norm.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                out.freq.store(self.freq.to_bits(), Ordering::Relaxed);
                self.crossings = 0;
                self.count = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_tone(band: HapticBand, hz: f32, amp: f32) -> (f32, f32) {
        let rate = 48_000.0;
        let mut dsp = Dsp::new(band, rate);
        let out = Analysis::default();
        let samples: Vec<f32> = (0..(rate as usize))
            .flat_map(|n| {
                let v = amp * (2.0 * std::f32::consts::PI * hz * n as f32 / rate).sin();
                [v, v]
            })
            .collect();
        dsp.process(samples.into_iter(), 2, &out);
        (f32::from_bits(out.env.load(Ordering::Relaxed)), f32::from_bits(out.freq.load(Ordering::Relaxed)))
    }

    #[test]
    fn rumble_in_band_is_felt_and_pitch_tracked() {
        let (env, freq) = run_tone(HapticBand::EngineAndRoad, 50.0, 0.5);
        assert!(env > 0.5, "env {env}");
        assert!((40.0..60.0).contains(&freq), "freq {freq}");
    }

    #[test]
    fn treble_is_ignored_and_silence_stays_still() {
        // A 2 kHz tone (voices, music) barely gets through the low-pass.
        let (env, _) = run_tone(HapticBand::EngineAndRoad, 2_000.0, 0.5);
        assert!(env < 0.3, "env {env}");
        let (env, _) = run_tone(HapticBand::EngineAndRoad, 50.0, 0.0);
        assert!(env < GATE, "env {env}");
    }
}
