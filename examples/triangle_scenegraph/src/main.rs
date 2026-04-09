use std::sync::Arc;

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use rig_app::{
    Application, CameraRig, RenderContext, StartupContext, UpdateContext,
    rig_assets::{
        MaterialAsset, MeshAsset, ShaderAsset, VertexAttribute, VertexFormat, VertexLayout,
    },
    rig_math::{BoundingSphere, Projection, Quat, Transform, Vec3},
    rig_scene::{CameraComponent, NodeId, Renderable},
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

const VERTICES: [Vertex; 3] = [
    Vertex {
        position: [0.0, 0.5, 0.0],
        color: [1.0, 0.0, 0.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.0],
        color: [0.0, 1.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.0],
        color: [0.0, 0.0, 1.0],
    },
];

const INDICES: [u16; 3] = [0, 1, 2];
const TRIANGLE_ROTATION_SPEED: f32 = std::f32::consts::FRAC_PI_2;

struct TriangleSceneApp {
    triangle: NodeId,
    camera: NodeId,
    camera_rig: CameraRig,
}

impl Application for TriangleSceneApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(rig_app::rig_render::TRIANGLE_SHADER),
        });
        let material = ctx.assets.add_material(MaterialAsset { shader, parameters: Default::default() });
        let mesh = ctx.assets.add_mesh(MeshAsset {
            vertex_layout: VertexLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                attributes: vec![
                    VertexAttribute {
                        shader_location: 0,
                        format: VertexFormat::Float32x3,
                        offset: 0,
                    },
                    VertexAttribute {
                        shader_location: 1,
                        format: VertexFormat::Float32x3,
                        offset: std::mem::size_of::<[f32; 3]>() as u64,
                    },
                ],
            },
            vertex_data: Arc::from(bytemuck::cast_slice(&VERTICES)),
            index_data: Arc::from(bytemuck::cast_slice(&INDICES)),
            local_bounds: BoundingSphere {
                center: Vec3::ZERO,
                radius: 0.75,
            },
        });

        let triangle = ctx.scene.create_node("triangle");
        ctx.scene
            .set_renderable(triangle, Renderable { mesh, material })?;

        let camera = ctx.scene.create_node("camera");
        ctx.scene.set_local_transform(
            camera,
            Transform {
                translation: Vec3::new(0.0, 0.0, 1.5),
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
                    far: 100.0,
                },
            },
        )?;

        Ok(Self {
            triangle,
            camera,
            camera_rig: CameraRig::default(),
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera);

        let mut triangle_transform = ctx.scene.local_transform(self.triangle)?;
        triangle_transform.rotation = (Quat::from_rotation_z(TRIANGLE_ROTATION_SPEED * dt)
            * triangle_transform.rotation)
            .normalize();
        ctx.scene
            .set_local_transform(self.triangle, triangle_transform)?;

        self.camera_rig.update(ctx, self.camera, dt)?;
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        ctx.renderer
            .render_scene(ctx.scene, ctx.assets, ctx.active_camera)?;
        Ok(())
    }
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<TriangleSceneApp>("Triangle SceneGraph")
}
