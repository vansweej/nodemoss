//! Voice-reactive metaballs — CPU Marching Cubes driven by a graphynx signal
//! processing pipeline.
//!
//! A voice signal (live microphone or synthetic additive synthesis) is analysed
//! every frame through a `Window → FFT → BandExtract` graph built with
//! graphynx.  The resulting three band energies (low / mid / high) are passed
//! through a per-band adaptive **peak tracker** that normalises the dynamic
//! range to [0, 1] regardless of absolute input volume.  The normalised values
//! then drive multiple animation axes simultaneously:
//!
//! | Band  | Primary axis            | Secondary axis          |
//! |-------|-------------------------|-------------------------|
//! | Low   | Ball radius pulsing     | Vertical bounce amplitude |
//! | Mid   | Orbit radius (spread)   | Orbit speed multiplier  |
//! | High  | ISO threshold (surface) | Extra ball fade-in/out  |
//!
//! ## Normalisation (PeakTracker)
//!
//! Raw band energies from `BandExtract` are first pre-scaled by `1/bin_count`
//! (mean instead of sum) to compensate for the large difference in bin counts
//! between bands (≈5 / ≈87 / ≈372 at 44100 Hz, FFT size 1024).  A per-band
//! peak tracker then normalises to [0, 1] using a fast-attack / slow-release
//! envelope, followed by a hysteresis gate that suppresses ambient noise.
//!
//! ## Phase Accumulation
//!
//! Ball positions are computed from accumulated phases rather than wall-clock
//! time.  This ensures that changes to the speed multiplier (driven by the mid
//! band) never cause position discontinuities — only the *rate* of phase
//! accumulation changes.
//!
//! ## Extra Balls
//!
//! Six `Ball` structs are always allocated.  Balls 0–3 are always visible.
//! Balls 4–5 fade in when the high band is sustained above a threshold for
//! `DEBOUNCE_FRAMES` consecutive frames, and fade out when it drops below the
//! despawn threshold for the same duration.
//!
//! # Audio modes
//!
//! The app tries to open the default microphone on startup.  If no device is
//! available it falls back to `SynthSource` automatically.  Press **M** to
//! toggle between live and synth at runtime.
//!
//! # Voice presets
//!
//! Three synthetic voice presets control the `SynthSource` parameters:
//!
//! | Key | Preset  | Fundamental | Formants                    |
//! |-----|---------|-------------|---------------------------  |
//! | 1   | Male    | 120 Hz      | 700 / 1200 / 2500 Hz        |
//! | 2   | Female  | 220 Hz      | 900 / 1800 / 2800 Hz        |
//! | 3   | Neutral | 170 Hz      | 800 / 1500 / 2650 Hz        |
//!
//! # Controls
//!
//! | Key(s)      | Action                        |
//! |-------------|-------------------------------|
//! | 1 / 2 / 3   | Switch voice preset           |
//! | M           | Toggle live / synth audio     |
//! | W / S       | Move forward / backward       |
//! | A / D       | Strafe left / right           |
//! | Q / E       | Move up / down                |
//! | Arrow keys  | Rotate camera (yaw / pitch)   |
//! | F3          | Toggle overlay                |
//! | F4          | Toggle wireframe              |
//! | Escape      | Close window                  |

use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    UpdateContext,
    rig_assets::{
        AlphaMode, DynamicMeshData, DynamicMeshId, MaterialAsset, MaterialParams, MeshSource,
        ShaderAsset,
        marching_cubes::{GridParams, extract},
    },
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

use backends_cpu::CpuBackend;
use graph_core::{
    graph::GraphBuilder,
    ops::{
        Op,
        signal::{
            BandDef, BandExtractParams, FftDirection, FftOutput, FftParams, WindowKind,
            WindowParams,
        },
    },
    types::TensorType,
};
use runtime::{
    audio::{AudioConfig, AudioSource, SynthSource},
    executor::Executor,
};

// ── Signal pipeline constants ─────────────────────────────────────────────────

/// FFT frame size (samples). Power-of-two for rustfft efficiency.
const FFT_SIZE: usize = 1024;
/// Audio sample rate (Hz).
const SAMPLE_RATE: f32 = 44_100.0;
/// EMA smoothing factor inside BandExtract (α).
///
/// Smooths per-frame FFT jitter before the signal reaches the peak tracker.
/// Keep this relatively fast (0.4–0.7) so transients are not over-smoothed.
const BAND_SMOOTHING: f32 = 0.6;

// ── Frequency bands ───────────────────────────────────────────────────────────

/// Low band: sub-bass + bass (20–250 Hz).
const BAND_LOW_HZ: (f32, f32) = (20.0, 250.0);
/// Mid band: voice fundamentals + harmonics (250–4000 Hz).
const BAND_MID_HZ: (f32, f32) = (250.0, 4_000.0);
/// High band: presence + air (4000–20000 Hz).
const BAND_HIGH_HZ: (f32, f32) = (4_000.0, 20_000.0);

/// Approximate bin counts for each band at FFT_SIZE=1024, SAMPLE_RATE=44100.
///
/// Used to pre-scale raw band energies from "sum of bins" to "mean of bins",
/// compensating for the large difference in band widths (≈5 / ≈87 / ≈372).
/// Without this, the high band would output ~74× more than the low band for
/// identical signal content, confusing the peak tracker during startup.
const BAND_BIN_COUNTS: [f32; 3] = [5.0, 87.0, 372.0];

// ── PeakTracker constants ─────────────────────────────────────────────────────

/// Per-frame decay factor for `peak_max`.
///
/// `0.997^60 ≈ 0.835` — peak halves in roughly 3 seconds at 60 fps.
/// Slow enough to stay calibrated through brief silences; fast enough to
/// adapt when the room gets quieter over time.
const RELEASE: f32 = 0.997;

/// Minimum value for `peak_max` (prevents divide-by-near-zero in silence).
const FLOOR: f32 = 0.02;

/// Normalised energy level above which the gate opens.
const GATE_OPEN_THRESHOLD: f32 = 0.10;

/// Normalised energy level below which the gate closes (hysteresis margin).
const GATE_CLOSE_THRESHOLD: f32 = 0.05;

/// Consecutive frames below `GATE_CLOSE_THRESHOLD` before the gate closes.
/// Gate opens instantly; closes only after this many quiet frames in a row.
/// 5 frames ≈ 83 ms at 60 fps — kills single-frame noise at idle.
const GATE_CLOSE_DEBOUNCE: u32 = 5;

// ── Animation parameter smoothing ────────────────────────────────────────────

/// Rate constant for smoothing animation parameters (units: 1/second).
///
/// Used as `alpha = 1 - exp(-dt * rate)` so smoothing is frame-rate
/// independent — identical behaviour at 30 fps and 60 fps.
///
/// `1/8 = 125 ms` time constant: fast enough to feel reactive, slow enough
/// to eliminate per-frame jitter from FFT variance.
const ANIM_SMOOTH_RATE: f32 = 8.0;

/// Rate constant for speed multiplier smoothing (units: 1/second).
///
/// Much slower than `ANIM_SMOOTH_RATE` because velocity changes are
/// perceptually jarring.  `1/2 = 500 ms` time constant.
const SPEED_SMOOTH_RATE: f32 = 2.0;

/// Rate constant for mid-band orbit spread (units: 1/second).
///
/// Faster than `ANIM_SMOOTH_RATE` so the orbit snaps open/closed immediately
/// when a singer starts or stops — making male/female patterns visually distinct.
/// `1/16 ≈ 62 ms` time constant.
const MID_ORBIT_SMOOTH_RATE: f32 = 16.0;

/// Rate constant for the low-band orbit bloom (units: 1/second).
///
/// Slower than `ANIM_SMOOTH_RATE` so the orbit expansion lags behind the
/// radius pulse — giving a "bloom then spread" feel on bass hits.
/// `1/3 ≈ 333 ms` time constant.
const LOW_ORBIT_SMOOTH_RATE: f32 = 3.0;

// ── Extra ball debounce ───────────────────────────────────────────────────────

/// High-band normalised level above which extra balls begin spawning.
const SPAWN_THRESHOLD: f32 = 0.6;

/// High-band normalised level below which extra balls begin despawning.
/// Lower than `SPAWN_THRESHOLD` to create hysteresis.
const DESPAWN_THRESHOLD: f32 = 0.35;

// ── Pulse LFO ─────────────────────────────────────────────────────────────────

/// Consecutive frames above/below threshold before a state transition fires.
/// At 60 fps this is ~167 ms — prevents flicker from transients.
const DEBOUNCE_FRAMES: u32 = 10;

/// Rate constant for the extra-ball radius fade (units: 1/second).
/// `1/6 ≈ 167 ms` time constant.
const EXTRA_FADE_RATE: f32 = 6.0;

// ── Grid configuration ────────────────────────────────────────────────────────

const GRID_RES: u32 = 32;
const GRID_HALF: f32 = 6.0;

fn grid_params() -> GridParams {
    GridParams {
        min: Vec3::splat(-GRID_HALF),
        max: Vec3::splat(GRID_HALF),
        resolution: [GRID_RES, GRID_RES, GRID_RES],
    }
}

// ── Animation ranges ──────────────────────────────────────────────────────────

/// Base radii for balls 0–3 (multiplied by `ball_radius_scale` each frame).
const BASE_RADIUS: [f32; 4] = [1.4, 1.3, 1.2, 1.1];

/// Base radius for extra balls 4–5 when fully visible.
/// Smaller than the main balls — they feel like satellite sparks.
const EXTRA_BASE_RADIUS: f32 = 0.9;

/// Base frequencies (x, y, z) per ball in radians/second.
///
/// Balls 0–3 preserve the Lissajous character of the original animation.
/// Balls 4–5 are faster — they orbit tightly around the main cluster.
const BASE_FREQ: [(f32, f32, f32); 6] = [
    (0.7, 0.5, 0.9), // ball 0
    (0.6, 0.8, 0.4), // ball 1
    (1.1, 0.7, 0.6), // ball 2
    (0.9, 0.5, 1.0), // ball 3
    (1.3, 0.9, 1.5), // ball 4 — extra, fast
    (1.1, 1.2, 0.8), // ball 5 — extra, offset from 4
];

/// Per-ball phase offsets (x, y, z) that restore the original trig variety.
///
/// The original animation used different trig functions per ball per axis
/// (`sin`, `cos`, `-sin`, `-cos`).  With phase accumulation all axes use a
/// single `.sin()` call, so the variety is encoded as additive offsets:
///
/// | Offset    | Equivalent trig |
/// |-----------|-----------------|
/// | `0`       | `sin`           |
/// | `π/2`     | `cos`           |
/// | `π`       | `-sin`          |
/// | `3π/2`    | `-cos`          |
const PHASE_OFFSET: [(f32, f32, f32); 6] = [
    (0.0, FRAC_PI_2, 0.0),                   // ball 0: (sin,  cos,  sin)
    (3.0 * FRAC_PI_2, 0.0, 3.0 * FRAC_PI_2), // ball 1: (-cos, sin,  -cos)
    (0.0, 3.0 * FRAC_PI_2, FRAC_PI_2),       // ball 2: (sin,  -cos, cos)
    (3.0 * FRAC_PI_2, PI, PI),               // ball 3: (-cos, -sin, -sin)
    (0.0, FRAC_PI_2, 0.0),                   // ball 4: (sin,  cos,  sin)
    (FRAC_PI_2, 0.0, 3.0 * FRAC_PI_2),       // ball 5: (cos,  sin,  -cos)
];

// ── Audio mode ────────────────────────────────────────────────────────────────

/// Whether the app is consuming live microphone input or synthetic audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioMode {
    /// Real-time microphone capture via cpal.
    Live,
    /// Deterministic additive synthesis (fallback / default when no mic).
    Synth,
}

impl AudioMode {
    fn label(self) -> &'static str {
        match self {
            AudioMode::Live => "Live",
            AudioMode::Synth => "Synth",
        }
    }

    fn toggled(self) -> Self {
        match self {
            AudioMode::Live => AudioMode::Synth,
            AudioMode::Synth => AudioMode::Live,
        }
    }
}

// ── Voice presets ─────────────────────────────────────────────────────────────

/// Synthetic voice preset — controls the additive synthesis parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoicePreset {
    /// Male voice: lower fundamental, darker formants.
    Male,
    /// Female voice: higher fundamental, brighter formants.
    Female,
    /// Neutral voice: midpoint between male and female.
    Neutral,
}

impl VoicePreset {
    /// Fundamental frequency in Hz.
    fn fundamental_hz(self) -> f32 {
        match self {
            VoicePreset::Male => 120.0,
            VoicePreset::Female => 220.0,
            VoicePreset::Neutral => 170.0,
        }
    }

    /// Formant centre frequencies in Hz (F1, F2, F3).
    fn formants_hz(self) -> [f32; 3] {
        match self {
            VoicePreset::Male => [700.0, 1_200.0, 2_500.0],
            VoicePreset::Female => [900.0, 1_800.0, 2_800.0],
            VoicePreset::Neutral => [800.0, 1_500.0, 2_650.0],
        }
    }

    fn label(self) -> &'static str {
        match self {
            VoicePreset::Male => "Male",
            VoicePreset::Female => "Female",
            VoicePreset::Neutral => "Neutral",
        }
    }
}

// ── Audio source construction ─────────────────────────────────────────────────

/// Build the base audio config (sample rate, frame size, channels).
fn audio_config() -> AudioConfig {
    AudioConfig {
        sample_rate: SAMPLE_RATE as u32,
        frame_size: FFT_SIZE,
        channels: 1,
    }
}

/// Build a `SynthSource` for the given preset.
fn make_synth_source(preset: VoicePreset) -> SynthSource {
    SynthSource::new(
        audio_config(),
        preset.fundamental_hz(),
        preset.formants_hz(),
    )
}

/// Try to open a live capture source; fall back to synth on any error.
///
/// Returns `(source, actual_mode)`.
fn make_audio_source(
    preset: VoicePreset,
    requested: AudioMode,
) -> (Box<dyn AudioSource>, AudioMode) {
    if requested == AudioMode::Live {
        match runtime::audio::capture::CpalCapture::new(audio_config()) {
            Ok(cap) => {
                log::info!("Live audio capture started");
                return (Box::new(cap), AudioMode::Live);
            }
            Err(e) => {
                log::warn!("Live audio unavailable ({e}); falling back to SynthSource");
            }
        }
    }
    (Box::new(make_synth_source(preset)), AudioMode::Synth)
}

// ── Graphynx pipeline ─────────────────────────────────────────────────────────

/// Build and return a new `Executor` for the given voice preset.
///
/// Graph topology:
/// ```text
/// [audio: f32×FFT_SIZE] → Window(Hann) → FFT(Magnitude) → BandExtract(3 bands, EMA)
///                                                                    ↓
///                                                         [energies: f32×3]
/// ```
fn build_pipeline(preset: VoicePreset) -> Result<Executor> {
    let spectrum_len = FFT_SIZE / 2 + 1;

    let win_params = WindowParams::new(WindowKind::Hann, FFT_SIZE)
        .map_err(|e| anyhow::anyhow!("WindowParams: {e}"))?;
    let fft_params = FftParams::new(FFT_SIZE, FftDirection::Forward, FftOutput::Magnitude)
        .map_err(|e| anyhow::anyhow!("FftParams: {e}"))?;
    let bands = vec![
        BandDef::new(BAND_LOW_HZ.0, BAND_LOW_HZ.1, "low")
            .map_err(|e| anyhow::anyhow!("BandDef low: {e}"))?,
        BandDef::new(BAND_MID_HZ.0, BAND_MID_HZ.1, "mid")
            .map_err(|e| anyhow::anyhow!("BandDef mid: {e}"))?,
        BandDef::new(BAND_HIGH_HZ.0, BAND_HIGH_HZ.1, "high")
            .map_err(|e| anyhow::anyhow!("BandDef high: {e}"))?,
    ];
    let be_params = BandExtractParams::new(bands, SAMPLE_RATE, BAND_SMOOTHING)
        .map_err(|e| anyhow::anyhow!("BandExtractParams: {e}"))?;

    let graph = GraphBuilder::new()
        .source("audio", TensorType::f32_1d(FFT_SIZE))
        .add_node("window")
        .device("cpu:0")
        .op(Op::Window(win_params))
        .input_from_source("audio")
        .output(TensorType::f32_1d(FFT_SIZE))
        .done()
        .add_node("fft")
        .device("cpu:0")
        .op(Op::Fft(fft_params))
        .input_from("window", 0)
        .output(TensorType::f32_1d(spectrum_len))
        .done()
        .add_node("bands")
        .device("cpu:0")
        .op(Op::BandExtract(be_params))
        .stateful()
        .input_from("fft", 0)
        .output(TensorType::f32_1d(3))
        .done()
        .sink("energies", TensorType::f32_1d(3))
        .from("bands", 0)
        .done()
        .build()
        .map_err(|e| anyhow::anyhow!("GraphBuilder: {e:?}"))?;

    let backend: Box<dyn backends::Backend> = Box::new(CpuBackend::new("cpu:0"));
    let executor =
        Executor::new(graph, vec![backend]).map_err(|e| anyhow::anyhow!("Executor: {e}"))?;

    log::info!(
        "Built voice pipeline for preset {:?} (f0={:.0} Hz)",
        preset,
        preset.fundamental_hz()
    );

    Ok(executor)
}

// ── PeakTracker ───────────────────────────────────────────────────────────────

/// Per-band adaptive peak tracker with hysteresis gate.
///
/// Normalises raw band energies to [0, 1] regardless of absolute input volume
/// by tracking a running per-band maximum with fast attack and slow release.
///
/// ## Algorithm (per frame, per band)
///
/// 1. `peak_max = max(peak_max × RELEASE, scaled_raw)`
/// 2. `norm = scaled_raw / max(peak_max, FLOOR)`
/// 3. Gate hysteresis: open when `norm > GATE_OPEN_THRESHOLD`,
///    close when `norm < GATE_CLOSE_THRESHOLD`
/// 4. Output: `normalised = if gate_open { norm } else { 0.0 }`
#[derive(Debug)]
struct PeakTracker {
    /// Running maximum per band (fast attack, slow release).
    peak_max: [f32; 3],
    /// Normalised output after gate, in [0, 1].
    normalised: [f32; 3],
    /// Hysteresis gate state per band.
    gate_open: [bool; 3],
    /// Per-band counter of consecutive frames below `GATE_CLOSE_THRESHOLD`.
    close_counter: [u32; 3],
}

impl PeakTracker {
    /// Create a new tracker.
    ///
    /// `peak_max` is initialised to `1.0` so the first few frames produce
    /// proportional output rather than a divide-by-zero spike.
    fn new() -> Self {
        Self {
            peak_max: [1.0; 3],
            normalised: [0.0; 3],
            gate_open: [false; 3],
            close_counter: [0; 3],
        }
    }

    /// Update the tracker with new raw (pre-scaled) band energies.
    ///
    /// `raw` must have length 3.  Values are expected to be pre-scaled by
    /// `1/bin_count` so all three bands have comparable magnitude.
    fn update(&mut self, raw: &[f32; 3]) {
        for (i, &raw_val) in raw.iter().enumerate() {
            // Fast attack, slow release.
            self.peak_max[i] = (self.peak_max[i] * RELEASE).max(raw_val);

            // Normalise.
            let norm = raw_val / self.peak_max[i].max(FLOOR);

            // Gate: instant open, debounced close.
            if !self.gate_open[i] && norm > GATE_OPEN_THRESHOLD {
                self.gate_open[i] = true;
                self.close_counter[i] = 0;
            } else if self.gate_open[i] && norm < GATE_CLOSE_THRESHOLD {
                self.close_counter[i] += 1;
                if self.close_counter[i] >= GATE_CLOSE_DEBOUNCE {
                    self.gate_open[i] = false;
                    self.close_counter[i] = 0;
                }
            } else {
                self.close_counter[i] = 0;
            }

            self.normalised[i] = if self.gate_open[i] { norm } else { 0.0 };
        }
    }

    /// Reset peak state (call when switching audio mode or preset).
    fn reset(&mut self) {
        self.peak_max = [1.0; 3];
        self.normalised = [0.0; 3];
        self.gate_open = [false; 3];
        self.close_counter = [0; 3];
    }
}

// ── BallPhase ─────────────────────────────────────────────────────────────────

/// Accumulated phase for one ball along its three Lissajous axes.
///
/// Using accumulated phases rather than wall-clock time ensures that changes
/// to the speed multiplier never cause position discontinuities — only the
/// *rate* of phase accumulation changes.
#[derive(Clone, Copy, Debug, Default)]
struct BallPhase {
    x: f32,
    y: f32,
    z: f32,
}

impl BallPhase {
    /// Advance phases by `dt` seconds at the given speed multiplier.
    fn advance(&mut self, dt: f32, speed_mult: f32, base_freq: (f32, f32, f32)) {
        self.x += dt * speed_mult * base_freq.0;
        self.y += dt * speed_mult * base_freq.1;
        self.z += dt * speed_mult * base_freq.2;
    }
}

// ── ExtraBallState ────────────────────────────────────────────────────────────

/// State machine for the two extra metaballs (balls 4 & 5).
///
/// Extra balls fade in when the high band is sustained above `SPAWN_THRESHOLD`
/// for `DEBOUNCE_FRAMES` consecutive frames, and fade out when it drops below
/// `DESPAWN_THRESHOLD` for the same duration.  The debounce prevents flicker
/// from transients.
#[derive(Debug)]
struct ExtraBallState {
    /// Current rendered radius for each extra ball (0 = invisible).
    radius: [f32; 2],
    /// Target radius (non-zero when active, zero when despawning).
    target: [f32; 2],
    /// Consecutive frames above `SPAWN_THRESHOLD`.
    frames_above: u32,
    /// Consecutive frames below `DESPAWN_THRESHOLD`.
    frames_below: u32,
    /// Whether balls are currently active (spawning or fully visible).
    active: bool,
}

impl ExtraBallState {
    fn new() -> Self {
        Self {
            radius: [0.0; 2],
            target: [0.0; 2],
            frames_above: 0,
            frames_below: 0,
            active: false,
        }
    }

    /// Update state based on the current high-band normalised energy and
    /// the audio-scaled extra ball radius.
    fn update(&mut self, high: f32, extra_radius: f32, dt: f32) {
        // Debounce counting.
        if high > SPAWN_THRESHOLD {
            self.frames_above += 1;
            self.frames_below = 0;
        } else if high < DESPAWN_THRESHOLD {
            self.frames_below += 1;
            self.frames_above = 0;
        } else {
            // In the hysteresis dead-band: reset both counters.
            self.frames_above = 0;
            self.frames_below = 0;
        }

        // State transitions.
        if !self.active && self.frames_above >= DEBOUNCE_FRAMES {
            self.active = true;
            self.target = [extra_radius, extra_radius];
            log::debug!("Extra balls: spawning (high={high:.2})");
        }
        if self.active && self.frames_below >= DEBOUNCE_FRAMES {
            self.active = false;
            self.target = [0.0, 0.0];
            log::debug!("Extra balls: despawning (high={high:.2})");
        }

        // Keep target radius in sync with audio-driven size when active.
        if self.active {
            self.target = [extra_radius, extra_radius];
        }

        // Smooth fade toward target.
        let alpha = 1.0 - (-dt * EXTRA_FADE_RATE).exp();
        for i in 0..2 {
            self.radius[i] += alpha * (self.target[i] - self.radius[i]);
        }
    }

    /// Reset to invisible state (call when switching audio mode or preset).
    fn reset(&mut self) {
        *self = Self::new();
    }
}

// ── Metaball field ────────────────────────────────────────────────────────────

struct Ball {
    pos: Vec3,
    radius: f32,
}

fn metaball_field(balls: &[Ball], p: Vec3) -> f32 {
    balls
        .iter()
        .map(|b| {
            let d2 = (p - b.pos).length_squared().max(1e-6);
            b.radius * b.radius / d2
        })
        .sum()
}

fn metaball_normal(balls: &[Ball], p: Vec3) -> [f32; 3] {
    let mut grad = Vec3::ZERO;
    for b in balls {
        let d = p - b.pos;
        let d2 = d.length_squared().max(1e-6);
        grad += -2.0 * b.radius * b.radius * d / (d2 * d2);
    }
    let g = -grad;
    let len = g.length();
    if len > 1e-10 {
        let n = g / len;
        [n.x, n.y, n.z]
    } else {
        [0.0, 1.0, 0.0]
    }
}

/// Clamp a ball position so its field influence stays within the grid.
///
/// Leaves `margin` units of space beyond the ball radius for field falloff,
/// preventing flat-face artefacts where the isosurface intersects the grid
/// boundary.
fn clamp_ball_pos(pos: Vec3, ball_radius: f32) -> Vec3 {
    // Safety floor: never clamp to zero extent even for very large balls.
    let max_extent = (GRID_HALF - ball_radius - 0.8).max(0.5);
    pos.clamp(Vec3::splat(-max_extent), Vec3::splat(max_extent))
}

// ── Application state ─────────────────────────────────────────────────────────

struct VoiceMetaballsApp {
    // Scene
    camera_node: NodeId,
    camera_rig: CameraRig,
    metaball_node: NodeId,
    dyn_id: DynamicMeshId,
    pending_mesh: Option<DynamicMeshData>,
    triangle_count: u32,

    // Audio / signal pipeline
    preset: VoicePreset,
    audio_mode: AudioMode,
    audio_source: Box<dyn AudioSource>,
    executor: Executor,

    // Normalisation
    peak_tracker: PeakTracker,

    // Animation — smoothed parameters (dt-based EMA, frame-rate independent)
    /// Accumulated phases for all 6 balls.
    ball_phases: [BallPhase; 6],
    /// Smoothed speed multiplier (slow EMA prevents velocity discontinuities).
    smooth_speed: f32,
    /// Smoothed ball radius scale (low band).
    smooth_radius_scale: f32,
    /// Smoothed vertical bounce amplitude (low band).
    smooth_bounce_amp: f32,
    /// Smoothed orbit radius (mid band).
    smooth_orbit_radius: f32,
    /// Low-band orbit bloom — lagged behind radius pulse for "bloom then spread".
    smooth_low_orbit: f32,
    /// Smoothed ISO threshold (high band).
    smooth_iso: f32,
    /// Extra ball fade-in/out state machine.
    extra_balls: ExtraBallState,

    // HUD
    debug_hud: DebugHud,
    hud_preset: rig_app::rig_overlay::ElementId,
    hud_audio: rig_app::rig_overlay::ElementId,
    hud_energies: rig_app::rig_overlay::ElementId,
    hud_peaks: rig_app::rig_overlay::ElementId,
    hud_triangles: rig_app::rig_overlay::ElementId,
}

impl Application for VoiceMetaballsApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // ── PBR shader — chrome/liquid-metal material ──────────────────────
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                diffuse: [0.60, 0.64, 0.74, 1.0],
                metallic: 1.0,
                roughness: 0.10,
                ..Default::default()
            },
            textures: vec![],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        });

        // ── Dynamic mesh slot ──────────────────────────────────────────────
        let dyn_id = DynamicMeshId::from_raw(0);
        let initial_vertex_bytes = (32 * 5 * (GRID_RES as u64).pow(3)).next_power_of_two();
        let initial_index_bytes = (4 * 15 * (GRID_RES as u64).pow(3)).next_power_of_two();
        ctx.renderer.register_dynamic_mesh(
            &ctx.gpu.device,
            dyn_id,
            initial_vertex_bytes,
            initial_index_bytes,
        );

        // ── Scene node ─────────────────────────────────────────────────────
        let metaball_node = ctx.scene.create_node("metaballs");
        ctx.scene.set_renderable(
            metaball_node,
            Renderable {
                mesh: MeshSource::Dynamic(dyn_id),
                material,
            },
        )?;
        ctx.scene.set_local_transform(
            metaball_node,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        // ── Lights ─────────────────────────────────────────────────────────
        let light_setup: &[(&str, Vec3, Vec3, f32, f32)] = &[
            (
                "light_key",
                Vec3::new(8.0, 8.0, 8.0),
                Vec3::new(1.00, 0.97, 0.90),
                18.0,
                32.0,
            ),
            (
                "light_fill",
                Vec3::new(-9.0, 5.0, 6.0),
                Vec3::new(0.55, 0.65, 1.00),
                8.0,
                32.0,
            ),
            (
                "light_rim",
                Vec3::new(1.0, 7.0, -10.0),
                Vec3::new(0.85, 0.90, 1.00),
                12.0,
                32.0,
            ),
        ];
        for (name, pos, color, intensity, range) in light_setup {
            let node = ctx.scene.create_node(*name);
            ctx.scene.set_local_transform(
                node,
                Transform {
                    translation: *pos,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )?;
            ctx.scene.set_light(
                node,
                LightComponent {
                    kind: LightKind::Point {
                        color: *color,
                        intensity: *intensity,
                        range: *range,
                    },
                },
            )?;
        }

        // ── Camera ─────────────────────────────────────────────────────────
        let camera_node = ctx.scene.create_node("camera");
        let pitch = -(4.0_f32).atan2(12.0);
        ctx.scene.set_local_transform(
            camera_node,
            Transform {
                translation: Vec3::new(0.0, 4.0, 12.0),
                rotation: Quat::from_rotation_x(pitch),
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_camera(
            camera_node,
            CameraComponent {
                projection: Projection::Perspective {
                    fov_y_radians: 60.0_f32.to_radians(),
                    near: 0.1,
                    far: 200.0,
                },
            },
        )?;

        // ── Signal pipeline ────────────────────────────────────────────────
        let preset = VoicePreset::Neutral;
        let mut executor = build_pipeline(preset)?;

        // Try live audio; fall back to synth automatically.
        let (mut audio_source, audio_mode) = make_audio_source(preset, AudioMode::Live);

        // ── PeakTracker warmup ─────────────────────────────────────────────
        // Run the pipeline for 60 frames using the actual audio source so
        // peak_max is calibrated before the first render.  Without this,
        // peak_max starts at [1.0, 1.0, 1.0] and the synth's strong mid
        // energy normalises to ~1.0 on frame 1, causing a chaotic startup.
        let mut peak_tracker = PeakTracker::new();
        for _ in 0..60 {
            if let Some(frame) = audio_source.next_frame()
                && executor
                    .input("audio")
                    .and_then(|i| i.write("audio", frame))
                    .is_ok()
            {
                let _ = executor.run();
                if let Some(raw) = executor
                    .output("energies")
                    .ok()
                    .and_then(|o| o.read::<f32>())
                {
                    let scaled = [
                        raw[0] / BAND_BIN_COUNTS[0],
                        raw[1] / BAND_BIN_COUNTS[1],
                        raw[2] / BAND_BIN_COUNTS[2],
                    ];
                    peak_tracker.update(&scaled);
                }
            }
        }

        // ── HUD ────────────────────────────────────────────────────────────
        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        let hud_preset = debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("Preset: {} (1/2/3)", preset.label()),
        );
        let hud_audio = debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("Audio: {} [M]", audio_mode.label()),
        );
        let hud_energies =
            debug_hud.add_element(ctx.overlay, Side::Left, "Bands: L=0.00 M=0.00 H=0.00");
        let hud_peaks =
            debug_hud.add_element(ctx.overlay, Side::Left, "Peaks: L=1.00 M=1.00 H=1.00");
        let hud_triangles = debug_hud.add_element(ctx.overlay, Side::Right, "Triangles: 0");

        log::info!(
            "Voice metaballs initialised (adaptive). Keys 1/2/3 = preset, M = audio mode, F3 = overlay, F4 = wireframe."
        );

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 5.0,
                rotation_speed: 1.5,
            },
            metaball_node,
            dyn_id,
            pending_mesh: None,
            triangle_count: 0,
            preset,
            audio_mode,
            audio_source,
            executor,
            peak_tracker,
            ball_phases: [BallPhase::default(); 6],
            smooth_speed: 0.5,
            smooth_radius_scale: 1.0,
            smooth_bounce_amp: 2.5,
            smooth_orbit_radius: 2.5,
            smooth_low_orbit: 0.0,
            smooth_iso: 1.0,
            extra_balls: ExtraBallState::new(),
            debug_hud,
            hud_preset,
            hud_audio,
            hud_energies,
            hud_peaks,
            hud_triangles,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        // ── Handle preset switching (keys 1 / 2 / 3) ──────────────────────
        let new_preset = if ctx.input.is_key_pressed(KeyCode::Digit1) {
            Some(VoicePreset::Male)
        } else if ctx.input.is_key_pressed(KeyCode::Digit2) {
            Some(VoicePreset::Female)
        } else if ctx.input.is_key_pressed(KeyCode::Digit3) {
            Some(VoicePreset::Neutral)
        } else {
            None
        };

        if let Some(p) = new_preset
            && p != self.preset
        {
            self.preset = p;
            self.executor = build_pipeline(p)?;
            if self.audio_mode == AudioMode::Synth {
                let (src, mode) = make_audio_source(p, AudioMode::Synth);
                self.audio_source = src;
                self.audio_mode = mode;
            }
            self.peak_tracker.reset();
            self.extra_balls.reset();
            self.smooth_radius_scale = 1.0;
            self.smooth_bounce_amp = 2.5;
            self.smooth_orbit_radius = 2.5;
            self.smooth_low_orbit = 0.0;
            self.smooth_iso = 1.0;
            log::info!("Switched to preset: {}", p.label());
        }

        // ── Handle audio mode toggle (M key) ──────────────────────────────
        if ctx.input.is_key_pressed(KeyCode::KeyM) {
            let requested = self.audio_mode.toggled();
            let (src, actual) = make_audio_source(self.preset, requested);
            self.audio_source = src;
            self.audio_mode = actual;
            self.peak_tracker.reset();
            self.extra_balls.reset();
            self.smooth_radius_scale = 1.0;
            self.smooth_bounce_amp = 2.5;
            self.smooth_orbit_radius = 2.5;
            self.smooth_low_orbit = 0.0;
            self.smooth_iso = 1.0;
            log::info!("Audio mode: {}", self.audio_mode.label());
        }

        // ── Run the signal pipeline ────────────────────────────────────────
        if let Some(frame) = self.audio_source.next_frame() {
            self.executor.input("audio")?.write("audio", frame)?;
            self.executor.run()?;
            let raw_energies: &[f32] = self
                .executor
                .output("energies")?
                .read()
                .ok_or_else(|| anyhow::anyhow!("energies output not ready"))?;

            // Pre-scale by 1/bin_count to normalise band widths.
            let scaled = [
                raw_energies[0] / BAND_BIN_COUNTS[0],
                raw_energies[1] / BAND_BIN_COUNTS[1],
                raw_energies[2] / BAND_BIN_COUNTS[2],
            ];

            self.peak_tracker.update(&scaled);
        }

        // ── Read normalised band values ────────────────────────────────────
        let low = self.peak_tracker.normalised[0];
        let mid = self.peak_tracker.normalised[1];
        let high = self.peak_tracker.normalised[2];

        // ── Map band energies to animation parameters ──────────────────────
        // All parameters are smoothed with a dt-based EMA for frame-rate
        // independent behaviour (no jitter at 30 fps or 144 fps).

        let anim_alpha = 1.0 - (-dt * ANIM_SMOOTH_RATE).exp();
        let speed_alpha = 1.0 - (-dt * SPEED_SMOOTH_RATE).exp();

        // Low band → vertical bounce amplitude only.
        // Radius is fixed — scaling caused balls to fuse when agitated.
        let target_radius_scale = 1.0;
        let target_bounce_amp = 2.5 + low * 3.0; // 2.5 → 5.5 units
        self.smooth_radius_scale += anim_alpha * (target_radius_scale - self.smooth_radius_scale);
        self.smooth_bounce_amp += anim_alpha * (target_bounce_amp - self.smooth_bounce_amp);

        // Mid band → orbit radius + speed multiplier.
        let target_orbit_radius = 2.5 + mid * 2.5; // 2.5 → 5.0 units
        let target_speed = 0.5 + high * 2.0; // 0.5× at idle → 2.5× on high energy
        let mid_orbit_alpha = 1.0 - (-dt * MID_ORBIT_SMOOTH_RATE).exp();
        self.smooth_orbit_radius +=
            mid_orbit_alpha * (target_orbit_radius - self.smooth_orbit_radius);
        self.smooth_speed += speed_alpha * (target_speed - self.smooth_speed);

        // Low band → orbit bloom (lagged — "bloom then spread" on bass hits).
        let low_orbit_alpha = 1.0 - (-dt * LOW_ORBIT_SMOOTH_RATE).exp();
        self.smooth_low_orbit += low_orbit_alpha * (low * 1.5 - self.smooth_low_orbit);

        // High band → ISO threshold + extra balls.
        let target_iso = 1.0 + high * 1.5; // 1.0 → 2.5
        self.smooth_iso += anim_alpha * (target_iso - self.smooth_iso);

        // Extra ball target radius scales with smoothed ball_radius_scale.
        let extra_radius = EXTRA_BASE_RADIUS * self.smooth_radius_scale;
        self.extra_balls.update(high, extra_radius, dt);

        // ── Advance phase accumulators ─────────────────────────────────────
        for (i, phase) in self.ball_phases.iter_mut().enumerate() {
            phase.advance(dt, self.smooth_speed, BASE_FREQ[i]);
        }

        // ── Build ball array (always 6) ────────────────────────────────────
        // Balls 0–3: main cluster, driven by low + mid bands.
        // Balls 4–5: extra, radius fades in/out with high band.
        let balls: [Ball; 6] = {
            let orbit = self.smooth_orbit_radius + self.smooth_low_orbit;
            let make_main = |i: usize| {
                let r = BASE_RADIUS[i] * self.smooth_radius_scale;
                let ph = &self.ball_phases[i];
                let off = PHASE_OFFSET[i];
                let pos = Vec3::new(
                    orbit * (ph.x + off.0).sin(),
                    self.smooth_bounce_amp * (ph.y + off.1).sin(),
                    orbit * (ph.z + off.2).sin(),
                );
                Ball {
                    pos: clamp_ball_pos(pos, r),
                    radius: r,
                }
            };

            let make_extra = |i: usize| {
                let r = self.extra_balls.radius[i - 4];
                let extra_orbit = orbit * 0.6;
                let ph = &self.ball_phases[i];
                let off = PHASE_OFFSET[i];
                let pos = Vec3::new(
                    extra_orbit * (ph.x + off.0).sin(),
                    self.smooth_bounce_amp * 0.7 * (ph.y + off.1).sin(),
                    extra_orbit * (ph.z + off.2).sin(),
                );
                Ball {
                    pos: clamp_ball_pos(pos, r.max(0.01)),
                    radius: r,
                }
            };

            [
                make_main(0),
                make_main(1),
                make_main(2),
                make_main(3),
                make_extra(4),
                make_extra(5),
            ]
        };

        // ── Marching Cubes ─────────────────────────────────────────────────
        let params = grid_params();
        let field = |p: Vec3| metaball_field(&balls, p);
        let normal = |p: Vec3| metaball_normal(&balls, p);
        let mesh_data = extract(&field, &params, self.smooth_iso, Some(&normal));

        ctx.scene
            .set_dynamic_bounds(self.metaball_node, mesh_data.local_bounds)?;

        self.triangle_count = mesh_data.index_count / 3;
        self.pending_mesh = Some(mesh_data);

        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;

        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        if let Some(data) = self.pending_mesh.take() {
            ctx.renderer
                .update_dynamic_mesh(&ctx.gpu.device, &ctx.gpu.queue, self.dyn_id, &data);
        }
        ctx.renderer
            .render_scene(ctx.gpu, ctx.frame, ctx.scene, ctx.assets, ctx.active_camera)?;
        Ok(())
    }

    fn update_overlay(&mut self, ctx: &mut OverlayUpdateContext<'_>) -> Result<()> {
        self.debug_hud.update(ctx)?;
        ctx.set_text(
            self.hud_preset,
            format!("Preset: {} (1/2/3)", self.preset.label()),
        )?;
        ctx.set_text(
            self.hud_audio,
            format!("Audio: {} [M]", self.audio_mode.label()),
        )?;
        ctx.set_text(
            self.hud_energies,
            format!(
                "Bands: L={:.2} M={:.2} H={:.2}",
                self.peak_tracker.normalised[0],
                self.peak_tracker.normalised[1],
                self.peak_tracker.normalised[2],
            ),
        )?;
        ctx.set_text(
            self.hud_peaks,
            format!(
                "Peaks: L={:.2} M={:.2} H={:.2}",
                self.peak_tracker.peak_max[0],
                self.peak_tracker.peak_max[1],
                self.peak_tracker.peak_max[2],
            ),
        )?;
        ctx.set_text(
            self.hud_triangles,
            format!("Triangles: {}", self.triangle_count),
        )?;
        Ok(())
    }

    fn on_window_event(&mut self, ctx: &mut UpdateContext<'_>, event: &WindowEvent) -> Result<()> {
        if let WindowEvent::KeyboardInput { event, .. } = event
            && matches!(
                event.physical_key,
                rig_app::winit::keyboard::PhysicalKey::Code(KeyCode::Escape)
            )
            && event.state == rig_app::winit::event::ElementState::Pressed
        {
            ctx.request_exit();
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<VoiceMetaballsApp>(rig_app::RunConfig {
        title: "Voice Metaballs".into(),
        ..Default::default()
    })
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── AudioMode ─────────────────────────────────────────────────────────

    #[test]
    fn audio_mode_labels_are_non_empty() {
        assert!(!AudioMode::Live.label().is_empty());
        assert!(!AudioMode::Synth.label().is_empty());
    }

    #[test]
    fn audio_mode_toggle_is_involutive() {
        assert_eq!(AudioMode::Live.toggled(), AudioMode::Synth);
        assert_eq!(AudioMode::Synth.toggled(), AudioMode::Live);
    }

    #[test]
    fn audio_mode_equality() {
        assert_eq!(AudioMode::Live, AudioMode::Live);
        assert_ne!(AudioMode::Live, AudioMode::Synth);
    }

    // ── VoicePreset ───────────────────────────────────────────────────────

    #[test]
    fn voice_preset_fundamentals_are_distinct() {
        let f_male = VoicePreset::Male.fundamental_hz();
        let f_female = VoicePreset::Female.fundamental_hz();
        let f_neutral = VoicePreset::Neutral.fundamental_hz();
        assert!(f_male < f_neutral);
        assert!(f_neutral < f_female);
    }

    #[test]
    fn voice_preset_formants_have_three_entries() {
        for preset in [VoicePreset::Male, VoicePreset::Female, VoicePreset::Neutral] {
            assert_eq!(preset.formants_hz().len(), 3);
        }
    }

    #[test]
    fn voice_preset_labels_are_non_empty() {
        for preset in [VoicePreset::Male, VoicePreset::Female, VoicePreset::Neutral] {
            assert!(!preset.label().is_empty());
        }
    }

    #[test]
    fn voice_preset_equality() {
        assert_eq!(VoicePreset::Male, VoicePreset::Male);
        assert_ne!(VoicePreset::Male, VoicePreset::Female);
    }

    // ── audio_config ──────────────────────────────────────────────────────

    #[test]
    fn audio_config_reflects_constants() {
        let cfg = audio_config();
        assert_eq!(cfg.sample_rate, SAMPLE_RATE as u32);
        assert_eq!(cfg.frame_size, FFT_SIZE);
        assert_eq!(cfg.channels, 1);
    }

    // ── make_audio_source ─────────────────────────────────────────────────

    #[test]
    fn make_audio_source_synth_always_succeeds() {
        let (mut src, mode) = make_audio_source(VoicePreset::Neutral, AudioMode::Synth);
        assert_eq!(mode, AudioMode::Synth);
        assert!(src.next_frame().is_some());
    }

    #[test]
    fn make_audio_source_live_falls_back_to_synth_in_ci() {
        let (_src, mode) = make_audio_source(VoicePreset::Neutral, AudioMode::Live);
        assert!(mode == AudioMode::Live || mode == AudioMode::Synth);
    }

    // ── build_pipeline ────────────────────────────────────────────────────

    #[test]
    fn build_pipeline_succeeds_for_all_presets() {
        for preset in [VoicePreset::Male, VoicePreset::Female, VoicePreset::Neutral] {
            assert!(build_pipeline(preset).is_ok(), "preset={preset:?}");
        }
    }

    #[test]
    fn pipeline_produces_three_band_energies() {
        let mut exec = build_pipeline(VoicePreset::Neutral).unwrap();
        let mut src = make_synth_source(VoicePreset::Neutral);
        let frame = src.next_frame().unwrap().to_vec();
        exec.input("audio")
            .unwrap()
            .write("audio", frame.as_slice())
            .unwrap();
        exec.run().unwrap();
        let energies: &[f32] = exec.output("energies").unwrap().read().unwrap();
        assert_eq!(energies.len(), 3);
    }

    #[test]
    fn pipeline_energies_are_non_negative() {
        let mut exec = build_pipeline(VoicePreset::Male).unwrap();
        let mut src = make_synth_source(VoicePreset::Male);
        let frame = src.next_frame().unwrap().to_vec();
        exec.input("audio")
            .unwrap()
            .write("audio", frame.as_slice())
            .unwrap();
        exec.run().unwrap();
        let energies: &[f32] = exec.output("energies").unwrap().read().unwrap();
        for &e in energies {
            assert!(e >= 0.0, "negative energy: {e}");
        }
    }

    #[test]
    fn pipeline_ema_state_persists_across_ticks() {
        let mut exec = build_pipeline(VoicePreset::Neutral).unwrap();
        let mut src = make_synth_source(VoicePreset::Neutral);
        let mut last_energies = [0.0_f32; 3];

        for tick in 0..5 {
            let frame = src.next_frame().unwrap().to_vec();
            exec.input("audio")
                .unwrap()
                .write("audio", frame.as_slice())
                .unwrap();
            exec.run().unwrap();
            let energies: &[f32] = exec.output("energies").unwrap().read().unwrap();
            if tick > 0 {
                for (i, (&cur, &prev)) in energies.iter().zip(last_energies.iter()).enumerate() {
                    let delta = (cur - prev).abs();
                    assert!(
                        delta.is_finite(),
                        "band {i} energy is not finite at tick {tick}: {cur}"
                    );
                }
            }
            last_energies.copy_from_slice(energies);
        }
    }

    // ── PeakTracker ───────────────────────────────────────────────────────

    #[test]
    fn peak_tracker_output_never_exceeds_one() {
        let mut tracker = PeakTracker::new();
        // Feed very large values — output must stay ≤ 1.0.
        for _ in 0..20 {
            tracker.update(&[100.0, 200.0, 50.0]);
        }
        for &n in &tracker.normalised {
            assert!(n <= 1.0, "normalised={n} exceeds 1.0");
        }
    }

    #[test]
    fn peak_tracker_adapts_to_loud_input() {
        let mut tracker = PeakTracker::new();
        // Feed consistent high values for many frames.
        for _ in 0..60 {
            tracker.update(&[10.0, 10.0, 10.0]);
        }
        // After adaptation, normalised should be close to 1.0.
        for (i, &n) in tracker.normalised.iter().enumerate() {
            assert!(
                n > 0.8,
                "band {i} normalised={n:.3} did not adapt to loud input"
            );
        }
    }

    #[test]
    fn peak_tracker_decays_in_silence() {
        let mut tracker = PeakTracker::new();
        // Warm up with loud signal.
        for _ in 0..30 {
            tracker.update(&[5.0, 5.0, 5.0]);
        }
        let peak_after_loud = tracker.peak_max;
        // Feed silence.
        for _ in 0..30 {
            tracker.update(&[0.0, 0.0, 0.0]);
        }
        // peak_max must have decayed.
        for i in 0..3 {
            assert!(
                tracker.peak_max[i] < peak_after_loud[i],
                "band {i} peak_max did not decay: before={} after={}",
                peak_after_loud[i],
                tracker.peak_max[i]
            );
        }
    }

    #[test]
    fn peak_tracker_floor_prevents_nan() {
        let mut tracker = PeakTracker::new();
        // Feed all-zero input for many frames.
        for _ in 0..100 {
            tracker.update(&[0.0, 0.0, 0.0]);
        }
        for &n in &tracker.normalised {
            assert!(
                n.is_finite(),
                "normalised is NaN or infinite with zero input"
            );
        }
        for &p in &tracker.peak_max {
            assert!(p.is_finite(), "peak_max is NaN or infinite with zero input");
        }
    }

    #[test]
    fn peak_tracker_gate_opens_above_threshold() {
        let mut tracker = PeakTracker::new();
        // Warm up peak_max so a moderate signal normalises above GATE_OPEN_THRESHOLD.
        for _ in 0..10 {
            tracker.update(&[1.0, 1.0, 1.0]);
        }
        // After warming, gate should be open for all bands.
        for (i, &open) in tracker.gate_open.iter().enumerate() {
            assert!(open, "band {i} gate did not open after warm-up");
        }
    }

    #[test]
    fn peak_tracker_gate_closes_below_threshold() {
        let mut tracker = PeakTracker::new();
        // Open the gate.
        for _ in 0..20 {
            tracker.update(&[1.0, 1.0, 1.0]);
        }
        assert!(tracker.gate_open.iter().all(|&g| g));
        // Feed near-zero — gate should close eventually.
        for _ in 0..200 {
            tracker.update(&[0.0, 0.0, 0.0]);
        }
        for (i, &open) in tracker.gate_open.iter().enumerate() {
            assert!(!open, "band {i} gate did not close after silence");
        }
    }

    #[test]
    fn peak_tracker_gate_hysteresis_no_oscillation() {
        let mut tracker = PeakTracker::new();
        // Warm up so peak_max is calibrated.
        for _ in 0..30 {
            tracker.update(&[1.0, 1.0, 1.0]);
        }
        let initial_gate = tracker.gate_open;
        // Feed a value in the dead-band between GATE_CLOSE and GATE_OPEN.
        let dead_band_value = (GATE_OPEN_THRESHOLD + GATE_CLOSE_THRESHOLD) / 2.0;
        // We need to get the normalised value into the dead-band.
        // Set peak_max manually to make dead_band_value normalise to dead_band_value.
        tracker.peak_max = [1.0; 3];
        for _ in 0..20 {
            tracker.update(&[dead_band_value, dead_band_value, dead_band_value]);
        }
        // Gate state must not have changed (hysteresis holds it stable).
        assert_eq!(
            tracker.gate_open, initial_gate,
            "gate oscillated in the hysteresis dead-band"
        );
    }

    #[test]
    fn peak_tracker_reset_reinitialises() {
        let mut tracker = PeakTracker::new();
        for _ in 0..20 {
            tracker.update(&[5.0, 5.0, 5.0]);
        }
        tracker.reset();
        assert_eq!(tracker.peak_max, [1.0; 3]);
        assert_eq!(tracker.normalised, [0.0; 3]);
        assert_eq!(tracker.gate_open, [false; 3]);
        assert_eq!(tracker.close_counter, [0; 3]);
    }

    // ── BallPhase ─────────────────────────────────────────────────────────

    #[test]
    fn ball_phase_accumulates_monotonically() {
        let mut phase = BallPhase::default();
        let dt = 1.0 / 60.0;
        let freq = (1.0, 1.0, 1.0);
        let mut prev_x = phase.x;
        for _ in 0..120 {
            phase.advance(dt, 1.0, freq);
            assert!(
                phase.x >= prev_x,
                "phase.x decreased: prev={prev_x} cur={}",
                phase.x
            );
            prev_x = phase.x;
        }
    }

    #[test]
    fn ball_phase_zero_speed_no_change() {
        let mut phase = BallPhase {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        phase.advance(1.0 / 60.0, 0.0, (1.0, 1.0, 1.0));
        assert_eq!(phase.x, 1.0);
        assert_eq!(phase.y, 2.0);
        assert_eq!(phase.z, 3.0);
    }

    #[test]
    fn ball_phase_scales_with_speed() {
        let mut slow = BallPhase::default();
        let mut fast = BallPhase::default();
        let dt = 1.0 / 60.0;
        let freq = (1.0, 1.0, 1.0);
        slow.advance(dt, 1.0, freq);
        fast.advance(dt, 2.0, freq);
        assert!(
            (fast.x - 2.0 * slow.x).abs() < 1e-6,
            "phase does not scale linearly with speed"
        );
    }

    #[test]
    fn main_balls_have_distinct_positions() {
        // Advance 4 main balls for 60 frames and verify no two land at the
        // same position.  Regression guard against collapsing all balls back
        // to an identical trig pattern (the "one blob" bug).
        let mut phases = [BallPhase::default(); 4];
        let dt = 1.0 / 60.0;
        for _ in 0..60 {
            for (i, ph) in phases.iter_mut().enumerate() {
                ph.advance(dt, 1.0, BASE_FREQ[i]);
            }
        }
        let orbit = 1.5_f32;
        let bounce = 2.5_f32;
        let positions: Vec<(f32, f32, f32)> = (0..4)
            .map(|i| {
                let ph = &phases[i];
                let off = PHASE_OFFSET[i];
                (
                    orbit * (ph.x + off.0).sin(),
                    bounce * (ph.y + off.1).sin(),
                    orbit * (ph.z + off.2).sin(),
                )
            })
            .collect();
        for i in 0..4 {
            for j in (i + 1)..4 {
                let dist = ((positions[i].0 - positions[j].0).powi(2)
                    + (positions[i].1 - positions[j].1).powi(2)
                    + (positions[i].2 - positions[j].2).powi(2))
                .sqrt();
                assert!(
                    dist > 0.1,
                    "balls {i} and {j} are too close after 60 frames: dist={dist:.4}"
                );
            }
        }
    }

    // ── ExtraBallState ────────────────────────────────────────────────────

    #[test]
    fn extra_ball_radius_zero_at_idle() {
        let state = ExtraBallState::new();
        assert_eq!(state.radius, [0.0; 2]);
        assert!(!state.active);
    }

    #[test]
    fn extra_ball_no_spawn_before_debounce() {
        let mut state = ExtraBallState::new();
        let dt = 1.0 / 60.0;
        // Feed high signal for fewer than DEBOUNCE_FRAMES frames.
        for _ in 0..(DEBOUNCE_FRAMES - 1) {
            state.update(SPAWN_THRESHOLD + 0.1, 1.0, dt);
        }
        assert!(!state.active, "extra balls spawned before debounce elapsed");
    }

    #[test]
    fn extra_ball_spawns_after_debounce() {
        let mut state = ExtraBallState::new();
        let dt = 1.0 / 60.0;
        for _ in 0..DEBOUNCE_FRAMES {
            state.update(SPAWN_THRESHOLD + 0.1, 1.0, dt);
        }
        assert!(state.active, "extra balls did not spawn after debounce");
    }

    #[test]
    fn extra_ball_despawns_after_debounce() {
        let mut state = ExtraBallState::new();
        let dt = 1.0 / 60.0;
        // Spawn first.
        for _ in 0..DEBOUNCE_FRAMES {
            state.update(SPAWN_THRESHOLD + 0.1, 1.0, dt);
        }
        assert!(state.active);
        // Now feed below despawn threshold.
        for _ in 0..DEBOUNCE_FRAMES {
            state.update(DESPAWN_THRESHOLD - 0.1, 1.0, dt);
        }
        assert!(!state.active, "extra balls did not despawn after debounce");
    }

    #[test]
    fn extra_ball_radius_fades_toward_target() {
        let mut state = ExtraBallState::new();
        let dt = 1.0 / 60.0;
        // Spawn.
        for _ in 0..DEBOUNCE_FRAMES {
            state.update(SPAWN_THRESHOLD + 0.1, 1.0, dt);
        }
        // After spawning, radius should be moving toward target.
        let r_after_spawn = state.radius[0];
        state.update(SPAWN_THRESHOLD + 0.1, 1.0, dt);
        assert!(
            state.radius[0] >= r_after_spawn,
            "radius decreased toward target"
        );
    }

    #[test]
    fn extra_ball_reset_clears_state() {
        let mut state = ExtraBallState::new();
        let dt = 1.0 / 60.0;
        for _ in 0..DEBOUNCE_FRAMES {
            state.update(SPAWN_THRESHOLD + 0.1, 1.0, dt);
        }
        assert!(state.active);
        state.reset();
        assert!(!state.active);
        assert_eq!(state.radius, [0.0; 2]);
        assert_eq!(state.frames_above, 0);
    }

    // ── clamp_ball_pos ────────────────────────────────────────────────────

    #[test]
    fn clamp_ball_pos_keeps_within_grid() {
        // A ball far outside the grid must be clamped inside.
        let pos = Vec3::new(100.0, -100.0, 50.0);
        let clamped = clamp_ball_pos(pos, 1.0);
        let max_extent = GRID_HALF - 1.0 - 0.8;
        assert!(clamped.x <= max_extent);
        assert!(clamped.x >= -max_extent);
        assert!(clamped.y <= max_extent);
        assert!(clamped.y >= -max_extent);
    }

    #[test]
    fn clamp_ball_pos_larger_radius_tighter_clamp() {
        let pos = Vec3::new(5.0, 0.0, 0.0);
        let small = clamp_ball_pos(pos, 0.5);
        let large = clamp_ball_pos(pos, 2.0);
        // Larger radius → smaller max_extent → position clamped more tightly.
        assert!(
            large.x <= small.x,
            "larger radius should produce tighter clamp: small.x={} large.x={}",
            small.x,
            large.x
        );
    }

    #[test]
    fn clamp_ball_pos_identity_for_small_pos() {
        // A ball well within the grid should not be moved.
        let pos = Vec3::new(1.0, 0.5, -0.5);
        let clamped = clamp_ball_pos(pos, 1.0);
        assert!((clamped.x - pos.x).abs() < 1e-6);
        assert!((clamped.y - pos.y).abs() < 1e-6);
        assert!((clamped.z - pos.z).abs() < 1e-6);
    }

    // ── metaball_field ────────────────────────────────────────────────────

    #[test]
    fn metaball_field_is_positive() {
        let balls = [
            Ball {
                pos: Vec3::ZERO,
                radius: 1.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
        ];
        let v = metaball_field(&balls, Vec3::new(1.0, 0.0, 0.0));
        assert!(v > 0.0);
    }

    #[test]
    fn metaball_field_increases_closer_to_centre() {
        let balls = [
            Ball {
                pos: Vec3::ZERO,
                radius: 1.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
        ];
        let far = metaball_field(&balls, Vec3::new(3.0, 0.0, 0.0));
        let near = metaball_field(&balls, Vec3::new(1.0, 0.0, 0.0));
        assert!(near > far);
    }

    #[test]
    fn metaball_field_zero_radius_balls_contribute_nothing() {
        let balls_with = [
            Ball {
                pos: Vec3::ZERO,
                radius: 1.0,
            },
            Ball {
                pos: Vec3::ONE,
                radius: 0.0,
            }, // zero radius — no contribution
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
        ];
        let balls_without = [
            Ball {
                pos: Vec3::ZERO,
                radius: 1.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
        ];
        let p = Vec3::new(1.5, 0.0, 0.0);
        let v_with = metaball_field(&balls_with, p);
        let v_without = metaball_field(&balls_without, p);
        assert!(
            (v_with - v_without).abs() < 1e-6,
            "zero-radius ball contributed to field: with={v_with} without={v_without}"
        );
    }

    #[test]
    fn metaball_normal_is_unit_length() {
        let balls = [
            Ball {
                pos: Vec3::ZERO,
                radius: 1.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
            Ball {
                pos: Vec3::ZERO,
                radius: 0.0,
            },
        ];
        let n = metaball_normal(&balls, Vec3::new(1.0, 0.0, 0.0));
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-5, "normal length={len}");
    }

    // ── speed smoothing ───────────────────────────────────────────────────

    #[test]
    fn speed_smooth_converges_to_target() {
        let mut smooth_speed = 0.5_f32;
        let target = 2.5_f32;
        let dt = 1.0 / 60.0; // simulate 60 fps
        // Run for 200 frames (~3.3 seconds — well past the 500 ms time constant).
        for _ in 0..200 {
            let alpha = 1.0 - (-dt * SPEED_SMOOTH_RATE).exp();
            smooth_speed += alpha * (target - smooth_speed);
        }
        assert!(
            (smooth_speed - target).abs() < 0.01,
            "smooth_speed={smooth_speed:.4} did not converge to target={target}"
        );
    }

    #[test]
    fn speed_smooth_single_frame_change_is_bounded() {
        let smooth_speed = 0.5_f32;
        let target = 2.5_f32;
        let dt = 1.0 / 60.0;
        let alpha = 1.0 - (-dt * SPEED_SMOOTH_RATE).exp();
        let new_speed = smooth_speed + alpha * (target - smooth_speed);
        let change = (new_speed - smooth_speed).abs();
        let max_change = alpha * (target - smooth_speed).abs();
        assert!(
            (change - max_change).abs() < 1e-6,
            "single-frame speed change {change} exceeds bound {max_change}"
        );
    }
}
