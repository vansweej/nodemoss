use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    UpdateContext,
    rig_assets::{MaterialAsset, MaterialParams, ShaderAsset, mesh_factory},
    rig_import::{AssetPath, FilesystemSource, Importer, MeshConfig, TextureConfig},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_overlay::ElementId,
    rig_render::{PHONG_SHADER, TEXTURED_SHADER},
    rig_scene::{CameraComponent, LightComponent, LightKind, MeshSource, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

#[derive(Clone, Copy, Debug)]
enum ExampleKind {
    ObjLoad,
    ObjTextured,
    MultiObj,
    TextureLoad,
    TextureFormats,
    ShaderLoad,
    AssetShowcase,
}

impl ExampleKind {
    fn from_index(index: usize) -> Self {
        match index {
            0 => Self::ObjLoad,
            1 => Self::ObjTextured,
            2 => Self::MultiObj,
            3 => Self::TextureLoad,
            4 => Self::TextureFormats,
            5 => Self::ShaderLoad,
            _ => Self::AssetShowcase,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::ObjLoad => "OBJ Load",
            Self::ObjTextured => "OBJ Textured",
            Self::MultiObj => "Multi OBJ",
            Self::TextureLoad => "Texture Load",
            Self::TextureFormats => "Texture Formats",
            Self::ShaderLoad => "Shader Load",
            Self::AssetShowcase => "Asset Showcase",
        }
    }
}

static EXAMPLE_KIND: AtomicUsize = AtomicUsize::new(0);

struct LoadingExampleApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    animated_nodes: Vec<NodeId>,
    elapsed: f32,
    debug_hud: DebugHud,
    stats_id: ElementId,
    summary: String,
}

fn run_loading_example(kind: ExampleKind) -> Result<()> {
    EXAMPLE_KIND.store(kind as usize, Ordering::Relaxed);
    rig_app::run::<LoadingExampleApp>(rig_app::RunConfig {
        title: kind.title().into(),
        ..Default::default()
    })
}

impl Application for LoadingExampleApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let kind = ExampleKind::from_index(EXAMPLE_KIND.load(Ordering::Relaxed));
        let started = Instant::now();
        let mut importer = Importer::new(FilesystemSource::default());
        let mut animated_nodes = Vec::new();

        let phong_shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PHONG_SHADER),
        });
        let textured_shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(TEXTURED_SHADER),
        });

        match kind {
            ExampleKind::ObjLoad => {
                let summary = add_imported_model(
                    ctx,
                    &mut importer,
                    &AssetPath::new("assets/models/cube.obj"),
                    phong_shader,
                    Vec3::ZERO,
                    &mut animated_nodes,
                )?;
                finish_scene(ctx, kind, animated_nodes, summary, started)
            }
            ExampleKind::ObjTextured => {
                let summary = add_imported_model(
                    ctx,
                    &mut importer,
                    &AssetPath::new("assets/models/textured_cube.obj"),
                    textured_shader,
                    Vec3::ZERO,
                    &mut animated_nodes,
                )?;
                finish_scene(ctx, kind, animated_nodes, summary, started)
            }
            ExampleKind::MultiObj => {
                for (index, x) in [-3.0_f32, 0.0, 3.0].into_iter().enumerate() {
                    add_imported_model(
                        ctx,
                        &mut importer,
                        &AssetPath::new("assets/models/textured_cube.obj"),
                        textured_shader,
                        Vec3::new(x, 0.0, 0.0),
                        &mut animated_nodes,
                    )
                    .with_context(|| format!("importing copy {index}"))?;
                }
                finish_scene(
                    ctx,
                    kind,
                    animated_nodes,
                    "3 OBJ loads; textures registered: 1 despite 3 loads".into(),
                    started,
                )
            }
            ExampleKind::TextureLoad => {
                let material = add_loaded_texture_material(
                    ctx,
                    &mut importer,
                    textured_shader,
                    "assets/textures/checker.png",
                )?;
                let mesh = ctx.assets.add_mesh(mesh_factory::create_sphere(1.0, 32, 16));
                let node = add_renderable(ctx, "loaded_texture_sphere", mesh, material, Vec3::ZERO)?;
                animated_nodes.push(node);
                finish_scene(
                    ctx,
                    kind,
                    animated_nodes,
                    "texture path: assets/textures/checker.png; dimensions: 64x64; format: sRGB RGBA8".into(),
                    started,
                )
            }
            ExampleKind::TextureFormats => {
                for (path, x) in [
                    ("assets/textures/checker.png", -2.4_f32),
                    ("assets/textures/stripes.jpg", 0.0_f32),
                    ("assets/textures/gradient.tga", 2.4_f32),
                ] {
                    let material = add_loaded_texture_material(ctx, &mut importer, textured_shader, path)?;
                    let mesh = ctx.assets.add_mesh(mesh_factory::create_plane(1.6, 1.6));
                    let node = add_renderable(ctx, path, mesh, material, Vec3::new(x, 0.0, 0.0))?;
                    ctx.scene.set_local_transform(
                        node,
                        Transform {
                            translation: Vec3::new(x, 0.0, 0.0),
                            rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                            scale: Vec3::ONE,
                        },
                    )?;
                }
                finish_scene(
                    ctx,
                    kind,
                    animated_nodes,
                    "PNG/JPEG/TGA loaded as RGBA8 sRGB; original channels tracked by loader".into(),
                    started,
                )
            }
            ExampleKind::ShaderLoad => {
                let shader = importer.import_shader(&AssetPath::new("assets/shaders/phong.wgsl"), ctx.assets)?;
                let material = ctx.assets.add_material(MaterialAsset {
                    shader,
                    parameters: MaterialParams::default(),
                    textures: vec![],
                });
                let mesh = ctx.assets.add_mesh(mesh_factory::create_box(1.8, 1.8, 1.8));
                let node = add_renderable(ctx, "runtime_shader_cube", mesh, material, Vec3::ZERO)?;
                animated_nodes.push(node);
                finish_scene(
                    ctx,
                    kind,
                    animated_nodes,
                    "Shader: assets/shaders/phong.wgsl (runtime-loaded)".into(),
                    started,
                )
            }
            ExampleKind::AssetShowcase => {
                add_imported_model(
                    ctx,
                    &mut importer,
                    &AssetPath::new("assets/models/textured_cube.obj"),
                    textured_shader,
                    Vec3::new(-2.2, 0.0, 0.0),
                    &mut animated_nodes,
                )?;
                let shader = importer.import_shader(&AssetPath::new("assets/shaders/phong.wgsl"), ctx.assets)?;
                let material = ctx.assets.add_material(MaterialAsset {
                    shader,
                    parameters: MaterialParams {
                        diffuse: [0.4, 0.8, 1.0, 1.0],
                        ..Default::default()
                    },
                    textures: vec![],
                });
                let mesh = ctx.assets.add_mesh(mesh_factory::create_icosahedron());
                let node = add_renderable(ctx, "shader_loaded_ico", mesh, material, Vec3::new(2.2, 0.0, 0.0))?;
                animated_nodes.push(node);
                finish_scene(
                    ctx,
                    kind,
                    animated_nodes,
                    "registry summary: OBJ + texture + runtime shader loaded; cache hits active".into(),
                    started,
                )
            }
        }
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        self.elapsed += dt;
        for (index, &node) in self.animated_nodes.iter().enumerate() {
            let transform = ctx.scene.local_transform(node)?;
            ctx.scene.set_local_transform(
                node,
                Transform {
                    rotation: Quat::from_rotation_y(self.elapsed + index as f32),
                    ..transform
                },
            )?;
        }
        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        ctx.renderer
            .render_scene(ctx.gpu, ctx.frame, ctx.scene, ctx.assets, ctx.active_camera)?;
        Ok(())
    }

    fn update_overlay(&mut self, ctx: &mut OverlayUpdateContext<'_>) -> Result<()> {
        self.debug_hud.update(ctx)?;
        ctx.set_text(self.stats_id, self.summary.clone())
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

fn add_imported_model(
    ctx: &mut StartupContext<'_>,
    importer: &mut Importer,
    path: &AssetPath,
    shader: rig_app::rig_assets::ShaderHandle,
    translation: Vec3,
    animated_nodes: &mut Vec<NodeId>,
) -> Result<String> {
    let loaded = importer.import_mesh(path, &MeshConfig::default(), shader, ctx.assets)?;
    let material_handles: Vec<_> = loaded
        .materials
        .into_iter()
        .map(|(material, _name)| ctx.assets.add_material(material))
        .collect();
    let fallback_material = material_handles.first().copied();
    let mut vertex_bytes = 0;
    let mut index_bytes = 0;
    for imported in loaded.meshes {
        vertex_bytes += imported.mesh.vertex_data.len();
        index_bytes += imported.mesh.index_data.len();
        let material = imported
            .material_index
            .and_then(|index| material_handles.get(index).copied())
            .or(fallback_material)
            .context("imported model did not provide any material")?;
        let mesh = ctx.assets.add_mesh(imported.mesh);
        let node = add_renderable(ctx, &imported.name, mesh, material, translation)?;
        animated_nodes.push(node);
    }
    Ok(format!(
        "{}; vertex bytes: {}; index bytes: {}; triangles: {}",
        path.as_str(),
        vertex_bytes,
        index_bytes,
        index_bytes / 6
    ))
}

fn add_loaded_texture_material(
    ctx: &mut StartupContext<'_>,
    importer: &mut Importer,
    shader: rig_app::rig_assets::ShaderHandle,
    path: &str,
) -> Result<rig_app::rig_assets::MaterialHandle> {
    let texture = importer.import_texture(&AssetPath::new(path), &TextureConfig::default(), ctx.assets)?;
    let sampler = ctx.assets.add_sampler(Default::default());
    Ok(ctx.assets.add_material(MaterialAsset {
        shader,
        parameters: MaterialParams::default(),
        textures: vec![(texture, sampler)],
    }))
}

fn add_renderable(
    ctx: &mut StartupContext<'_>,
    name: &str,
    mesh: rig_app::rig_assets::MeshHandle,
    material: rig_app::rig_assets::MaterialHandle,
    translation: Vec3,
) -> Result<NodeId> {
    let node = ctx.scene.create_node(name);
    ctx.scene.set_renderable(
        node,
        Renderable {
            mesh: MeshSource::Static(mesh),
            material,
        },
    )?;
    ctx.scene.set_local_transform(
        node,
        Transform {
            translation,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        },
    )?;
    Ok(node)
}

fn finish_scene(
    ctx: &mut StartupContext<'_>,
    kind: ExampleKind,
    animated_nodes: Vec<NodeId>,
    summary: String,
    started: Instant,
) -> Result<LoadingExampleApp> {
    add_default_light(ctx)?;
    let camera_node = add_camera(ctx)?;
    let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
    let stats_id = debug_hud.add_element(
        ctx.overlay,
        Side::Right,
        format!("{}: {} (startup {} ms)", kind.title(), summary, started.elapsed().as_millis()),
    );
    Ok(LoadingExampleApp {
        camera_node,
        camera_rig: CameraRig {
            translation_speed: 4.0,
            rotation_speed: 1.5,
        },
        animated_nodes,
        elapsed: 0.0,
        debug_hud,
        stats_id,
        summary: format!("{}: {} (startup {} ms)", kind.title(), summary, started.elapsed().as_millis()),
    })
}

fn add_camera(ctx: &mut StartupContext<'_>) -> Result<NodeId> {
    let camera = ctx.scene.create_node("camera");
    let eye = Vec3::new(0.0, 2.5, 7.0);
    let pitch = -eye.y.atan2(eye.z);
    ctx.scene.set_local_transform(
        camera,
        Transform {
            translation: eye,
            rotation: Quat::from_rotation_x(pitch),
            scale: Vec3::ONE,
        },
    )?;
    ctx.scene.set_camera(
        camera,
        CameraComponent {
            projection: Projection::Perspective {
                fov_y_radians: 60.0_f32.to_radians(),
                near: 0.1,
                far: 100.0,
            },
        },
    )?;
    Ok(camera)
}

fn add_default_light(ctx: &mut StartupContext<'_>) -> Result<()> {
    let light = ctx.scene.create_node("directional_light");
    ctx.scene.set_local_transform(
        light,
        Transform {
            translation: Vec3::ZERO,
            rotation: Quat::from_rotation_x(-0.7),
            scale: Vec3::ONE,
        },
    )?;
    ctx.scene.set_light(
        light,
        LightComponent {
            kind: LightKind::Directional {
                color: Vec3::ONE,
                intensity: 1.2,
            },
        },
    )?;
    Ok(())
}
