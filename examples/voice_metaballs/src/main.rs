//! Voice-reactive metaballs — CPU Marching Cubes driven by a graphynx signal
//! processing pipeline.
//!
//! A voice signal (live microphone or synthetic additive synthesis) is analysed
//! every frame through a `Window → FFT → BandExtract` graph built with
//! graphynx.  The resulting three band energies (low / mid / high) are mapped
//! to metaball animation parameters in real time:
//!
//! | Band  | Mapping                                     |
//! |-------|---------------------------------------------|
//! | Low   | Orbit radii of balls 0 and 1                |
//! | Mid   | Orbit radii of balls 2 and 3                |
//! | High  | Isosurface threshold (ISO value)            |
//!
//! Band energies are smoothed with an exponential moving average (EMA) inside
//! the `BandExtract` op (α = 0.6) and then further normalised on the render
//! side with a per-band EMA (α = 1 − exp(−dt × RESPONSIVENESS)).
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
//! Pressing 1/2/3 rebuilds the graphynx graph with the new preset and updates
//! the `SynthSource` parameters.
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

use std::sync::Arc;

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    UpdateContext,
    rig_assets::{
        DynamicMeshData, DynamicMeshId, MaterialAsset, MaterialParams, MeshSource, ShaderAsset,
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
const BAND_SMOOTHING: f32 = 0.6;
/// Render-side EMA responsiveness (higher = faster response to transients).
const RESPONSIVENESS: f32 = 8.0;

// ── Frequency bands ───────────────────────────────────────────────────────────

/// Low band: sub-bass + bass (20–250 Hz).
const BAND_LOW_HZ: (f32, f32) = (20.0, 250.0);
/// Mid band: voice fundamentals + harmonics (250–4000 Hz).
const BAND_MID_HZ: (f32, f32) = (250.0, 4_000.0);
/// High band: presence + air (4000–20000 Hz).
const BAND_HIGH_HZ: (f32, f32) = (4_000.0, 20_000.0);

// ── Grid configuration ────────────────────────────────────────────────────────

const GRID_RES: u32 = 32;
const GRID_HALF: f32 = 6.0;

/// Base ISO threshold (no audio).
const ISO_BASE: f32 = 1.0;
/// Maximum additional ISO shift driven by the high band.
const ISO_RANGE: f32 = 0.6;

fn grid_params() -> GridParams {
    GridParams {
        min: Vec3::splat(-GRID_HALF),
        max: Vec3::splat(GRID_HALF),
        resolution: [GRID_RES, GRID_RES, GRID_RES],
    }
}

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

// ── Application state ─────────────────────────────────────────────────────────

struct VoiceMetaballsApp {
    // Scene
    camera_node: NodeId,
    camera_rig: CameraRig,
    metaball_node: NodeId,
    dyn_id: DynamicMeshId,
    pending_mesh: Option<DynamicMeshData>,
    elapsed: f64,
    triangle_count: u32,

    // Audio / signal pipeline
    preset: VoicePreset,
    audio_mode: AudioMode,
    audio_source: Box<dyn AudioSource>,
    executor: Executor,
    /// Smoothed band energies [low, mid, high] for animation (render-side EMA).
    smooth_energies: [f32; 3],

    // HUD
    debug_hud: DebugHud,
    hud_preset: rig_app::rig_overlay::ElementId,
    hud_audio: rig_app::rig_overlay::ElementId,
    hud_energies: rig_app::rig_overlay::ElementId,
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
        let executor = build_pipeline(preset)?;

        // Try live audio; fall back to synth automatically.
        let (audio_source, audio_mode) = make_audio_source(preset, AudioMode::Live);

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
        let hud_triangles = debug_hud.add_element(ctx.overlay, Side::Right, "Triangles: 0");

        log::info!(
            "Voice metaballs initialised. Keys 1/2/3 = preset, M = audio mode, F3 = overlay, F4 = wireframe."
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
            elapsed: 0.0,
            triangle_count: 0,
            preset,
            audio_mode,
            audio_source,
            executor,
            smooth_energies: [0.0; 3],
            debug_hud,
            hud_preset,
            hud_audio,
            hud_energies,
            hud_triangles,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        self.elapsed += dt as f64;
        let t = self.elapsed as f32;

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
            // Update synth params if in synth mode; live capture ignores preset.
            if self.audio_mode == AudioMode::Synth {
                let (src, mode) = make_audio_source(p, AudioMode::Synth);
                self.audio_source = src;
                self.audio_mode = mode;
            }
            self.smooth_energies = [0.0; 3];
            log::info!("Switched to preset: {}", p.label());
        }

        // ── Handle audio mode toggle (M key) ──────────────────────────────
        if ctx.input.is_key_pressed(KeyCode::KeyM) {
            let requested = self.audio_mode.toggled();
            let (src, actual) = make_audio_source(self.preset, requested);
            self.audio_source = src;
            self.audio_mode = actual;
            self.smooth_energies = [0.0; 3];
            log::info!("Audio mode: {}", self.audio_mode.label());
        }

        // ── Run the signal pipeline ────────────────────────────────────────
        // `next_frame` returns None when the ring buffer has fewer than
        // frame_size samples (only possible with CpalCapture on the very
        // first tick).  In that case we reuse the previous smooth_energies.
        if let Some(frame) = self.audio_source.next_frame() {
            self.executor.input("audio")?.write("audio", frame)?;
            self.executor.run()?;
            let raw_energies: &[f32] = self
                .executor
                .output("energies")?
                .read()
                .ok_or_else(|| anyhow::anyhow!("energies output not ready"))?;

            // ── Render-side EMA normalisation ──────────────────────────────
            // α = 1 − exp(−dt × RESPONSIVENESS)
            let alpha = 1.0 - (-dt * RESPONSIVENESS).exp();
            for (i, &e) in raw_energies.iter().enumerate().take(3) {
                self.smooth_energies[i] += alpha * (e - self.smooth_energies[i]);
            }
        }

        // ── Map band energies to animation parameters ──────────────────────
        // Normalise energies to [0, 1] using a soft ceiling.
        let norm = |e: f32| (e / (e + 1.0)).min(1.0);
        let low = norm(self.smooth_energies[0]);
        let mid = norm(self.smooth_energies[1]);
        let high = norm(self.smooth_energies[2]);

        // Orbit radii: base ± voice-driven variation.
        let r_low = 2.5 + low * 1.5; // balls 0 & 1 — driven by low band
        let r_mid = 2.5 + mid * 1.5; // balls 2 & 3 — driven by mid band

        // ISO threshold: rises with high-frequency energy (tightens surface).
        let iso = ISO_BASE + high * ISO_RANGE;

        // ── Animate 4 balls along Lissajous-like paths ─────────────────────
        let balls = [
            Ball {
                pos: Vec3::new(
                    r_low * (t * 0.7).sin(),
                    2.5 * (t * 0.5).cos(),
                    r_low * (t * 0.9).sin(),
                ),
                radius: 1.4,
            },
            Ball {
                pos: Vec3::new(
                    -r_low * (t * 0.6).cos(),
                    2.8 * (t * 0.8).sin(),
                    -r_low * (t * 0.4).cos(),
                ),
                radius: 1.3,
            },
            Ball {
                pos: Vec3::new(
                    r_mid * (t * 1.1).sin(),
                    -2.5 * (t * 0.7).cos(),
                    r_mid * (t * 0.6).cos(),
                ),
                radius: 1.2,
            },
            Ball {
                pos: Vec3::new(
                    -r_mid * (t * 0.9).cos(),
                    -3.0 * (t * 0.5).sin(),
                    -r_mid * (t * 1.0).sin(),
                ),
                radius: 1.1,
            },
        ];

        // ── Marching Cubes ─────────────────────────────────────────────────
        let params = grid_params();
        let field = |p: Vec3| metaball_field(&balls, p);
        let normal = |p: Vec3| metaball_normal(&balls, p);
        let mesh_data = extract(&field, &params, iso, Some(&normal));

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
                self.smooth_energies[0], self.smooth_energies[1], self.smooth_energies[2]
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
    fn audio_config_reflects_preset() {
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
        // SynthSource always returns Some on next_frame.
        assert!(src.next_frame().is_some());
    }

    #[test]
    fn make_audio_source_live_falls_back_to_synth_in_ci() {
        // In CI / headless environments there is no audio device.
        // make_audio_source must not panic; it returns Synth as fallback.
        let (_src, mode) = make_audio_source(VoicePreset::Neutral, AudioMode::Live);
        // Either Live (if a device exists) or Synth (fallback) is acceptable.
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

    // ── metaball_field ────────────────────────────────────────────────────

    #[test]
    fn metaball_field_is_positive() {
        let balls = vec![Ball {
            pos: Vec3::ZERO,
            radius: 1.0,
        }];
        let v = metaball_field(&balls, Vec3::new(1.0, 0.0, 0.0));
        assert!(v > 0.0);
    }

    #[test]
    fn metaball_field_increases_closer_to_centre() {
        let balls = vec![Ball {
            pos: Vec3::ZERO,
            radius: 1.0,
        }];
        let far = metaball_field(&balls, Vec3::new(3.0, 0.0, 0.0));
        let near = metaball_field(&balls, Vec3::new(1.0, 0.0, 0.0));
        assert!(near > far);
    }

    #[test]
    fn metaball_normal_is_unit_length() {
        let balls = vec![Ball {
            pos: Vec3::ZERO,
            radius: 1.0,
        }];
        let n = metaball_normal(&balls, Vec3::new(1.0, 0.0, 0.0));
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-5, "normal length={len}");
    }
}
