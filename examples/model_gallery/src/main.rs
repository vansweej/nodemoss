//! CLI-driven 3D model viewer.
//!
//! Loads a single model from the asset library, auto-scales it to fit a 2-unit
//! bounding sphere, and renders it with either Phong shading or its diffuse texture.
//!
//! # Usage
//!
//!     cargo run -p model_gallery              # default: teapot
//!     cargo run -p model_gallery -- bunny
//!     cargo run -p model_gallery -- spot      # textured
//!     cargo run -p model_gallery -- --help
//!
//! # Controls
//!
//! | Input      | Action                  |
//! |------------|-------------------------|
//! | LMB drag   | Orbit camera            |
//! | RMB drag   | Dolly (zoom in/out)     |
//! | W / S      | Move forward / backward |
//! | A / D      | Strafe left / right     |
//! | Q / E      | Move up / down          |
//! | Arrow keys | Orbit camera            |
//! | F3         | Toggle overlay          |
//! | Escape     | Close window            |

use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use rig_app::{
    Application, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext, TrackBall,
    UpdateContext,
    rig_assets::{AlphaMode, MaterialAsset, MaterialParams, ShaderAsset},
    rig_import::{AssetPath, FilesystemSource, Importer, MeshConfig},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_overlay::ElementId,
    rig_render::{PHONG_SHADER, TEXTURED_SHADER},
    rig_scene::{CameraComponent, LightComponent, LightKind, MeshSource, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

const TARGET_RADIUS: f32 = 2.0;
const KEYBOARD_DOLLY_SPEED: f32 = 4.0;
const KEYBOARD_ORBIT_SPEED: f32 = 1.5;
const KEYBOARD_PAN_SPEED: f32 = 4.0;

#[derive(Clone, Copy, Debug)]
struct ModelSpec {
    name: &'static str,
    path: &'static str,
    textured: bool,
}

const MODELS: &[ModelSpec] = &[
    ModelSpec {
        name: "teapot",
        path: "assets/models/teapot.obj",
        textured: false,
    },
    ModelSpec {
        name: "bunny",
        path: "assets/models/bunny.obj",
        textured: false,
    },
    ModelSpec {
        name: "buddha",
        path: "assets/models/buddha.obj",
        textured: false,
    },
    ModelSpec {
        name: "dragon",
        path: "assets/models/dragon.obj",
        textured: false,
    },
    ModelSpec {
        name: "armadillo",
        path: "assets/models/armadillo.obj",
        textured: false,
    },
    ModelSpec {
        name: "suzanne",
        path: "assets/models/suzanne.obj",
        textured: false,
    },
    ModelSpec {
        name: "nefertiti",
        path: "assets/models/nefertiti.obj",
        textured: false,
    },
    ModelSpec {
        name: "spot",
        path: "assets/models/spot/spot.obj",
        textured: true,
    },
    ModelSpec {
        name: "ogre",
        path: "assets/models/ogre/ogre.obj",
        textured: true,
    },
    ModelSpec {
        name: "bob",
        path: "assets/models/bob/bob.obj",
        textured: true,
    },
    ModelSpec {
        name: "blub",
        path: "assets/models/blub/blub.obj",
        textured: true,
    },
];

static SELECTED_MODEL: OnceLock<ModelSpec> = OnceLock::new();

struct ModelGalleryApp {
    camera_node: NodeId,
    trackball: TrackBall,
    debug_hud: DebugHud,
    stats_id: ElementId,
    overlay_text: String,
}

enum CliAction {
    Help,
    Run(ModelSpec),
}

fn parse_cli_action() -> Result<CliAction> {
    let Some(name) = std::env::args().nth(1) else {
        return Ok(CliAction::Run(MODELS[0]));
    };

    if is_help_arg(&name) {
        return Ok(CliAction::Help);
    }

    MODELS
        .iter()
        .copied()
        .find(|model| model.name.eq_ignore_ascii_case(&name))
        .map(CliAction::Run)
        .ok_or_else(|| anyhow::anyhow!(usage(&name)))
}

fn is_help_arg(name: &str) -> bool {
    matches!(name, "--help" | "-h")
}

fn usage(name: &str) -> String {
    let names = MODELS
        .iter()
        .map(|model| model.name)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "unknown model '{name}'. Available models: {names}\nRun `cargo run -p model_gallery -- --help` for usage."
    )
}

fn help_text() -> String {
    let models = MODELS
        .iter()
        .map(|model| {
            let kind = if model.textured {
                "textured"
            } else {
                "geometry"
            };
            let default_suffix = if model.name == MODELS[0].name {
                " (default)"
            } else {
                ""
            };
            format!("    {:<12} {kind}{default_suffix}", model.name)
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "model_gallery — CLI-driven 3D model viewer\n\n{usage}\n\nMODELS:\n{models}\n\n{controls}",
        usage = HELP_USAGE,
        controls = HELP_CONTROLS
    )
}

const HELP_USAGE: &str = "USAGE:
    cargo run -p model_gallery
    cargo run -p model_gallery -- MODEL
    cargo run -p model_gallery -- --help";

const HELP_CONTROLS: &str = "CONTROLS:
    LMB drag     Orbit camera
    RMB drag     Dolly (zoom in/out)
    W / S        Move forward / backward
    A / D        Strafe left / right
    Q / E        Move up / down
    Arrow keys   Orbit camera
    F3           Toggle overlay
    Escape       Close window";

fn print_help() {
    println!("{}", help_text());
}

fn update_keyboard_controls(trackball: &mut TrackBall, input: &rig_app::InputState, dt: f32) {
    let yaw = key_axis(input, KeyCode::ArrowRight, KeyCode::ArrowLeft) * KEYBOARD_ORBIT_SPEED * dt;
    let pitch = key_axis(input, KeyCode::ArrowDown, KeyCode::ArrowUp) * KEYBOARD_ORBIT_SPEED * dt;
    if yaw != 0.0 || pitch != 0.0 {
        trackball.orbit_by(yaw, pitch);
    }

    let dolly = key_axis(input, KeyCode::KeyW, KeyCode::KeyS) * KEYBOARD_DOLLY_SPEED * dt;
    if dolly != 0.0 {
        trackball.dolly_by(dolly);
    }

    let right = key_axis(input, KeyCode::KeyA, KeyCode::KeyD) * KEYBOARD_PAN_SPEED * dt;
    let up = key_axis(input, KeyCode::KeyQ, KeyCode::KeyE) * KEYBOARD_PAN_SPEED * dt;
    if right != 0.0 || up != 0.0 {
        trackball.pan_by(right, up);
    }
}

fn key_axis(input: &rig_app::InputState, negative: KeyCode, positive: KeyCode) -> f32 {
    let negative = input.is_key_pressed(negative) as i8;
    let positive = input.is_key_pressed(positive) as i8;
    (positive - negative) as f32
}

impl Application for ModelGalleryApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let spec = *SELECTED_MODEL
            .get()
            .context("model selection was not initialised")?;
        let started = Instant::now();

        let mut importer = Importer::new(FilesystemSource::default());
        let shader_source = if spec.textured {
            TEXTURED_SHADER
        } else {
            PHONG_SHADER
        };
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(shader_source),
        });
        let fallback_material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: fallback_params(spec.textured),
            textures: Vec::new(),
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        });

        let loaded = importer.import_mesh(
            &AssetPath::new(spec.path),
            &MeshConfig::default(),
            shader,
            ctx.assets,
        )?;
        let radius = loaded.bounds.radius.max(f32::EPSILON);
        let scale = TARGET_RADIUS / radius;
        let offset = -loaded.bounds.center * scale;

        let material_handles = loaded
            .materials
            .into_iter()
            .map(|(material, _name)| ctx.assets.add_material(material))
            .collect::<Vec<_>>();

        let orbit_target = ctx.scene.create_node(format!("{}_orbit_target", spec.name));
        let model_root = ctx.scene.create_node(spec.name);
        ctx.scene.set_local_transform(
            model_root,
            Transform {
                translation: offset,
                rotation: Quat::IDENTITY,
                scale: Vec3::splat(scale),
            },
        )?;
        ctx.scene.attach_child(orbit_target, model_root)?;

        let mut vertex_count = 0_usize;
        let mut triangle_count = 0_usize;
        for imported in loaded.meshes {
            vertex_count += imported.mesh.vertex_data.len() / 32;
            triangle_count +=
                imported.mesh.index_data.len() / index_size(imported.mesh.index_format) / 3;
            let material = imported
                .material_index
                .and_then(|index| material_handles.get(index).copied())
                .unwrap_or(fallback_material);
            let mesh = ctx.assets.add_mesh(imported.mesh);
            let node = ctx.scene.create_node(&imported.name);
            ctx.scene.set_renderable(
                node,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )?;
            ctx.scene.attach_child(model_root, node)?;
        }

        add_light(ctx)?;
        let camera_node = add_camera(ctx)?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        let overlay_text = format!(
            "Model: {}\nTriangles: {}\nVertices: {}\nBounds radius: {:.3}\nLoad time: {} ms",
            spec.name,
            triangle_count,
            vertex_count,
            loaded.bounds.radius,
            started.elapsed().as_millis()
        );
        let stats_id = debug_hud.add_element(ctx.overlay, Side::Right, overlay_text.clone());

        Ok(Self {
            camera_node,
            trackball: TrackBall::new(orbit_target, TARGET_RADIUS * 3.0),
            debug_hud,
            stats_id,
            overlay_text,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera_node);
        update_keyboard_controls(&mut self.trackball, ctx.input, dt);
        self.trackball
            .update(ctx.input, ctx.scene, self.camera_node, dt)?;
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        ctx.renderer
            .render_scene(ctx.gpu, ctx.frame, ctx.scene, ctx.assets, ctx.active_camera)?;
        Ok(())
    }

    fn update_overlay(&mut self, ctx: &mut OverlayUpdateContext<'_>) -> Result<()> {
        self.debug_hud.update(ctx)?;
        ctx.set_text(self.stats_id, self.overlay_text.clone())
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

fn fallback_params(textured: bool) -> MaterialParams {
    if textured {
        MaterialParams::default()
    } else {
        MaterialParams {
            ambient: [0.16, 0.16, 0.15, 1.0],
            diffuse: [0.8, 0.8, 0.75, 1.0],
            specular: [0.4, 0.4, 0.4, 32.0],
            ..Default::default()
        }
    }
}

fn index_size(format: rig_app::rig_assets::IndexFormat) -> usize {
    match format {
        rig_app::rig_assets::IndexFormat::Uint16 => 2,
        rig_app::rig_assets::IndexFormat::Uint32 => 4,
    }
}

fn add_camera(ctx: &mut StartupContext<'_>) -> Result<NodeId> {
    let camera = ctx.scene.create_node("camera");
    let eye = Vec3::new(0.0, 0.0, TARGET_RADIUS * 3.0);
    ctx.scene.set_local_transform(
        camera,
        Transform {
            translation: eye,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        },
    )?;
    ctx.scene.set_camera(
        camera,
        CameraComponent {
            projection: Projection::Perspective {
                fov_y_radians: 60.0_f32.to_radians(),
                near: 0.1,
                far: 200.0,
            },
        },
    )?;
    Ok(camera)
}

fn add_light(ctx: &mut StartupContext<'_>) -> Result<()> {
    let light = ctx.scene.create_node("key_light");
    ctx.scene.set_local_transform(
        light,
        Transform {
            translation: Vec3::ZERO,
            rotation: Quat::from_rotation_x(-0.7) * Quat::from_rotation_y(-0.5),
            scale: Vec3::ONE,
        },
    )?;
    ctx.scene.set_light(
        light,
        LightComponent {
            kind: LightKind::Directional {
                color: Vec3::ONE,
                intensity: 1.4,
            },
        },
    )?;
    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();
    let spec = match parse_cli_action() {
        Ok(CliAction::Help) => {
            print_help();
            return Ok(());
        }
        Ok(CliAction::Run(spec)) => spec,
        Err(err) => {
            eprintln!("{err}");
            bail!(err);
        }
    };
    SELECTED_MODEL
        .set(spec)
        .map_err(|_| anyhow::anyhow!("model selection was already initialised"))?;
    rig_app::run::<ModelGalleryApp>(rig_app::RunConfig {
        title: format!("Model Gallery — {}", spec.name),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_help_flags() {
        assert!(is_help_arg("--help"));
        assert!(is_help_arg("-h"));
        assert!(!is_help_arg("bunny"));
    }

    #[test]
    fn help_text_lists_every_model() {
        let help = help_text();

        for model in MODELS {
            assert!(help.contains(model.name));
        }
    }

    #[test]
    fn help_text_lists_mouse_and_keyboard_controls() {
        let help = help_text();

        assert!(help.contains("LMB drag"));
        assert!(help.contains("W / S"));
        assert!(help.contains("Arrow keys"));
    }
}
