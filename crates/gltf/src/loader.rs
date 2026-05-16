//! Top-level glTF loading entry point.

use std::path::Path;

use rig_assets::{
    AnimationClipHandle, AssetStore, MaterialHandle, MeshHandle, MeshSource, MorphTargetHandle,
    ShaderHandle, SkinAssetHandle, SkinWeightsHandle,
};
use rig_scene::{NodeId, Renderable, SceneGraph};

use crate::error::{GltfError, Result};
use crate::{animations, cameras, lights, materials, meshes, nodes, skins, textures};

/// Engine-side handles created while loading a glTF file.
///
/// The loader mutates the caller's [`SceneGraph`] and [`AssetStore`] directly;
/// this struct records the handles needed for follow-up runtime systems such as
/// animation playback, CPU skinning, morph evaluation, or UI inspection.
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
    /// Morph target handles per primitive, parallel to `meshes`.
    pub morph_targets: Vec<Vec<Option<MorphTargetHandle>>>,
    /// Skinned renderable primitives with enough handles for runtime CPU skinning.
    pub skinned_primitives: Vec<SkinnedPrimitive>,
}

/// Descriptor for one loaded primitive that can be evaluated through CPU skinning.
///
/// A descriptor is emitted only when the source glTF node references a skin and
/// the corresponding primitive has skin weight attributes. The renderable starts
/// as a static mesh; examples can replace it with a dynamic mesh and drive it
/// with `rig_skin::SkinEvaluator`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinnedPrimitive {
    /// Scene node that owns the renderable for this primitive.
    pub node: NodeId,
    /// Rest-pose mesh asset.
    pub mesh: MeshHandle,
    /// Skin asset containing joint names and inverse bind matrices.
    pub skin: SkinAssetHandle,
    /// Per-vertex skinning weights for this primitive.
    pub skin_weights: SkinWeightsHandle,
    /// Material assigned to the primitive renderable.
    pub material: MaterialHandle,
    /// Primitive index within the source glTF mesh.
    pub primitive_index: usize,
}

/// Scene selection mode for multi-scene glTF documents.
///
/// [`SceneSelection::Default`] uses the document default scene and falls back to
/// the first scene when the default scene is omitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneSelection {
    /// Load the document's default scene, or the first scene if no default is set.
    Default,
    /// Load a scene by glTF scene index.
    Index(usize),
    /// Load a scene by exact glTF scene name.
    Name(String),
}

/// Load the default scene from a glTF or GLB file.
///
/// This is a convenience wrapper around [`load_gltf_scene`] with
/// [`SceneSelection::Default`]. The provided shader is assigned to all adapted
/// materials, including the default material used for primitives without a glTF
/// material.
pub fn load_gltf(
    path: impl AsRef<Path>,
    shader: ShaderHandle,
    scene: &mut SceneGraph,
    store: &mut AssetStore,
) -> Result<LoadedGltf> {
    load_gltf_scene(path, SceneSelection::Default, shader, scene, store)
}

/// Load a selected scene from a glTF or GLB file.
///
/// Assets are registered in `store`, nodes and scene-facing components are
/// added to `scene`, and the returned [`LoadedGltf`] records handles created by
/// the load. Import errors include the source path for clearer CLI diagnostics.
pub fn load_gltf_scene(
    path: impl AsRef<Path>,
    scene_selection: SceneSelection,
    shader: ShaderHandle,
    scene: &mut SceneGraph,
    store: &mut AssetStore,
) -> Result<LoadedGltf> {
    let path = path.as_ref();
    let (document, buffers, images) = gltf::import(path).map_err(|source| GltfError::LoadFile {
        path: path.display().to_string(),
        source,
    })?;
    let gltf_scene = resolve_scene(&document, &scene_selection)?;

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
    let (node_map, root_nodes) = nodes::adapt_nodes(&document, gltf_scene, scene);

    let mut mesh_handles = Vec::new();
    let mut skin_weight_handles = Vec::new();
    let mut morph_target_handles = Vec::new();
    for gltf_mesh in document.meshes() {
        let adapted = meshes::adapt_mesh(&gltf_mesh, &buffers, store)?;
        let mut mesh_group = Vec::with_capacity(adapted.len());
        let mut morph_group = Vec::with_capacity(adapted.len());
        for primitive in &adapted {
            mesh_group.push(primitive.mesh);
            morph_group.push(primitive.morph_targets);
        }
        let mut weights_group = Vec::with_capacity(adapted.len());
        for primitive in gltf_mesh.primitives() {
            weights_group.push(skins::adapt_skin_weights(&primitive, &buffers, store)?);
        }
        mesh_handles.push(mesh_group);
        skin_weight_handles.push(weights_group);
        morph_target_handles.push(morph_group);
    }

    if material_handles.is_empty() {
        material_handles.push(default_material);
    }
    let skins = skins::adapt_skins(&document, &buffers, &node_map, scene, store)?;
    let skinned_primitives = wire_renderables(
        RenderableWireContext {
            document: &document,
            node_map: &node_map,
            mesh_handles: &mesh_handles,
            material_handles: &material_handles,
            skin_weight_handles: &skin_weight_handles,
            skin_handles: &skins,
            default_material,
        },
        scene,
    );

    cameras::adapt_cameras(&document, &node_map, scene);
    lights::adapt_lights(&document, &node_map, scene);
    let animations = animations::adapt_animations(&document, &buffers, &node_map, scene, store)?;

    Ok(LoadedGltf {
        root_nodes,
        node_map,
        animations,
        skins,
        meshes: mesh_handles,
        materials: material_handles,
        skin_weights: skin_weight_handles,
        morph_targets: morph_target_handles,
        skinned_primitives,
    })
}

fn resolve_scene<'a>(
    document: &'a gltf::Document,
    selection: &SceneSelection,
) -> Result<gltf::Scene<'a>> {
    match selection {
        SceneSelection::Default => document
            .default_scene()
            .or_else(|| document.scenes().next())
            .ok_or_else(|| GltfError::SceneNotFound {
                description: "default scene (file contains no scenes)".to_string(),
            }),
        SceneSelection::Index(index) => document
            .scenes()
            .find(|scene| scene.index() == *index)
            .ok_or_else(|| GltfError::SceneNotFound {
                description: format!("scene index {index}"),
            }),
        SceneSelection::Name(name) => document
            .scenes()
            .find(|scene| scene.name() == Some(name.as_str()))
            .ok_or_else(|| GltfError::SceneNotFound {
                description: format!("scene named '{name}'"),
            }),
    }
}

struct RenderableWireContext<'a> {
    document: &'a gltf::Document,
    node_map: &'a [Option<NodeId>],
    mesh_handles: &'a [Vec<MeshHandle>],
    material_handles: &'a [MaterialHandle],
    skin_weight_handles: &'a [Vec<Option<SkinWeightsHandle>>],
    skin_handles: &'a [SkinAssetHandle],
    default_material: MaterialHandle,
}

fn wire_renderables(
    context: RenderableWireContext<'_>,
    scene: &mut SceneGraph,
) -> Vec<SkinnedPrimitive> {
    let mut skinned_primitives = Vec::new();
    for node in context.document.nodes() {
        let Some(gltf_mesh) = node.mesh() else {
            continue;
        };
        let Some(node_id) = context.node_map.get(node.index()).copied().flatten() else {
            continue;
        };
        let Some(mesh_group) = context.mesh_handles.get(gltf_mesh.index()) else {
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
                .and_then(|index| context.material_handles.get(index).copied())
                .unwrap_or(context.default_material);

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

            let skin = node
                .skin()
                .and_then(|skin| context.skin_handles.get(skin.index()).copied());
            let skin_weights = context
                .skin_weight_handles
                .get(gltf_mesh.index())
                .and_then(|group| group.get(primitive_index))
                .copied()
                .flatten();
            if let (Some(skin), Some(skin_weights)) = (skin, skin_weights) {
                skinned_primitives.push(SkinnedPrimitive {
                    node: render_node,
                    mesh: mesh_handle,
                    skin,
                    skin_weights,
                    material,
                    primitive_index,
                });
            }
        }
    }
    skinned_primitives
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_scene_uses_default_scene() {
        let gltf = multi_scene_gltf();

        let scene = resolve_scene(&gltf.document, &SceneSelection::Default).expect("scene found");

        assert_eq!(scene.index(), 1);
        assert_eq!(scene.name(), Some("second"));
    }

    #[test]
    fn resolve_scene_selects_by_index() {
        let gltf = multi_scene_gltf();

        let scene = resolve_scene(&gltf.document, &SceneSelection::Index(0)).expect("scene found");

        assert_eq!(scene.index(), 0);
        assert_eq!(scene.name(), Some("first"));
    }

    #[test]
    fn resolve_scene_selects_by_name() {
        let gltf = multi_scene_gltf();

        let scene = resolve_scene(&gltf.document, &SceneSelection::Name("second".to_string()))
            .expect("scene found");

        assert_eq!(scene.index(), 1);
    }

    #[test]
    fn resolve_scene_reports_missing_index() {
        let gltf = multi_scene_gltf();

        let error = resolve_scene(&gltf.document, &SceneSelection::Index(99))
            .expect_err("scene should be missing");

        assert!(matches!(error, GltfError::SceneNotFound { .. }));
        assert!(error.to_string().contains("scene index 99"));
    }

    #[test]
    fn resolve_scene_reports_missing_name() {
        let gltf = multi_scene_gltf();

        let error = resolve_scene(&gltf.document, &SceneSelection::Name("missing".to_string()))
            .expect_err("scene should be missing");

        assert!(matches!(error, GltfError::SceneNotFound { .. }));
        assert!(error.to_string().contains("scene named 'missing'"));
    }

    #[test]
    fn wire_renderables_returns_skinned_primitive_descriptors() {
        let gltf = gltf::Gltf::from_slice(
            br#"{
                "asset": { "version": "2.0" },
                "buffers": [ { "byteLength": 12 } ],
                "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": 12 } ],
                "accessors": [
                    {
                        "bufferView": 0,
                        "componentType": 5126,
                        "count": 1,
                        "type": "VEC3",
                        "min": [0.0, 0.0, 0.0],
                        "max": [0.0, 0.0, 0.0]
                    }
                ],
                "materials": [ {} ],
                "meshes": [
                    { "primitives": [ { "attributes": { "POSITION": 0 }, "material": 0 } ] }
                ],
                "nodes": [
                    { "name": "mesh_node", "mesh": 0, "skin": 0 },
                    { "name": "joint" }
                ],
                "skins": [ { "joints": [1] } ],
                "scenes": [ { "nodes": [0] } ]
            }"#,
        )
        .expect("valid skinned glTF");
        let mut scene = SceneGraph::new();
        let mesh_node = scene.create_node("mesh_node");
        let joint_node = scene.create_node("joint");
        let node_map = vec![Some(mesh_node), Some(joint_node)];
        let mesh = MeshHandle::from_raw(2);
        let material = MaterialHandle::from_raw(3);
        let skin = SkinAssetHandle::from_raw(4);
        let skin_weights = SkinWeightsHandle::from_raw(5);

        let mesh_handles = [vec![mesh]];
        let material_handles = [material];
        let skin_weight_handles = [vec![Some(skin_weights)]];
        let skin_handles = [skin];
        let skinned = wire_renderables(
            RenderableWireContext {
                document: &gltf.document,
                node_map: &node_map,
                mesh_handles: &mesh_handles,
                material_handles: &material_handles,
                skin_weight_handles: &skin_weight_handles,
                skin_handles: &skin_handles,
                default_material: material,
            },
            &mut scene,
        );

        assert_eq!(
            skinned,
            vec![SkinnedPrimitive {
                node: mesh_node,
                mesh,
                skin,
                skin_weights,
                material,
                primitive_index: 0,
            }]
        );
        assert_eq!(
            scene.renderable(mesh_node).unwrap().copied(),
            Some(Renderable {
                mesh: MeshSource::Static(mesh),
                material,
            })
        );
    }

    fn multi_scene_gltf() -> gltf::Gltf {
        gltf::Gltf::from_slice(
            br#"{
                "asset": { "version": "2.0" },
                "nodes": [ { "name": "root" } ],
                "scenes": [
                    { "name": "first", "nodes": [0] },
                    { "name": "second", "nodes": [0] }
                ],
                "scene": 1
            }"#,
        )
        .expect("valid multi-scene glTF")
    }
}
