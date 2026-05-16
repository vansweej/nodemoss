//! Top-level glTF loading entry point.

use std::path::Path;

use rig_assets::{
    AnimationClipHandle, AssetStore, MaterialHandle, MeshHandle, MeshSource, ShaderHandle,
    SkinAssetHandle, SkinWeightsHandle,
};
use rig_scene::{NodeId, Renderable, SceneGraph};

use crate::error::Result;
use crate::{animations, cameras, lights, materials, meshes, nodes, skins, textures};

/// Result of loading a glTF file into the engine.
pub struct LoadedGltf {
    /// Top-level scene nodes (roots of the glTF scene hierarchy).
    pub root_nodes: Vec<NodeId>,
    /// All created nodes, indexed by glTF node index. `None` for unused slots.
    pub node_map: Vec<Option<NodeId>>,
    /// Animation clip handles, one per glTF animation (in document order).
    pub animations: Vec<AnimationClipHandle>,
    /// Skin asset handles, one per glTF skin (in document order).
    pub skins: Vec<SkinAssetHandle>,
    /// Mesh handles grouped by glTF mesh index, then primitive index.
    pub meshes: Vec<Vec<MeshHandle>>,
    /// Material handles, one per glTF material (in document order).
    pub materials: Vec<MaterialHandle>,
    /// Skin weights handles per primitive, parallel to `meshes`.
    pub skin_weights: Vec<Vec<Option<SkinWeightsHandle>>>,
}

/// Load a glTF or GLB file and adapt its contents into the engine.
pub fn load_gltf(
    path: impl AsRef<Path>,
    shader: ShaderHandle,
    scene: &mut SceneGraph,
    store: &mut AssetStore,
) -> Result<LoadedGltf> {
    let (document, buffers, images) = gltf::import(path.as_ref())?;

    let image_handles = textures::adapt_images(&images, store);
    let sampler_handles = textures::adapt_samplers(&document, store);
    let default_sampler = textures::default_sampler(store);
    let mut material_handles = materials::adapt_materials(
        &document,
        &image_handles,
        &sampler_handles,
        default_sampler,
        shader,
        store,
    );
    let default_material = materials::default_material(shader, store);
    let (node_map, root_nodes) = nodes::adapt_nodes(&document, scene);

    let mut mesh_handles = Vec::new();
    let mut skin_weight_handles = Vec::new();
    for gltf_mesh in document.meshes() {
        let adapted = meshes::adapt_mesh(&gltf_mesh, &buffers, store)?;
        let mut mesh_group = Vec::with_capacity(adapted.len());
        let mut weights_group = Vec::with_capacity(adapted.len());
        for (primitive, (mesh_handle, _)) in gltf_mesh.primitives().zip(adapted.iter()) {
            mesh_group.push(*mesh_handle);
            weights_group.push(skins::adapt_skin_weights(&primitive, &buffers, store)?);
        }
        mesh_handles.push(mesh_group);
        skin_weight_handles.push(weights_group);
    }

    if material_handles.is_empty() {
        material_handles.push(default_material);
    }
    wire_renderables(
        &document,
        &node_map,
        &mesh_handles,
        &material_handles,
        default_material,
        scene,
    );

    cameras::adapt_cameras(&document, &node_map, scene);
    lights::adapt_lights(&document, &node_map, scene);
    let animations = animations::adapt_animations(&document, &buffers, &node_map, scene, store)?;
    let skins = skins::adapt_skins(&document, &buffers, &node_map, scene, store)?;

    Ok(LoadedGltf {
        root_nodes,
        node_map,
        animations,
        skins,
        meshes: mesh_handles,
        materials: material_handles,
        skin_weights: skin_weight_handles,
    })
}

fn wire_renderables(
    document: &gltf::Document,
    node_map: &[Option<NodeId>],
    mesh_handles: &[Vec<MeshHandle>],
    material_handles: &[MaterialHandle],
    default_material: MaterialHandle,
    scene: &mut SceneGraph,
) {
    for node in document.nodes() {
        let Some(gltf_mesh) = node.mesh() else {
            continue;
        };
        let Some(node_id) = node_map.get(node.index()).copied().flatten() else {
            continue;
        };
        let Some(mesh_group) = mesh_handles.get(gltf_mesh.index()) else {
            continue;
        };
        let primitive_count = meshes::primitive_count(&gltf_mesh);
        for (primitive_index, primitive) in gltf_mesh.primitives().enumerate() {
            let Some(mesh_handle) = mesh_group.get(primitive_index).copied() else {
                continue;
            };
            let material = primitive
                .material()
                .index()
                .and_then(|index| material_handles.get(index).copied())
                .unwrap_or(default_material);

            let render_node = if primitive_count == 1 {
                node_id
            } else {
                let parent_name = scene.node_name(node_id).unwrap_or("node");
                let child = scene.create_node(format!("{parent_name}_prim_{primitive_index}"));
                let _ = scene.attach_child(node_id, child);
                child
            };
            let _ = scene.set_renderable(
                render_node,
                Renderable {
                    mesh: MeshSource::Static(mesh_handle),
                    material,
                },
            );
        }
    }
}
