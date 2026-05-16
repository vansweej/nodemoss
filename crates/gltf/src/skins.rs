//! glTF skin and skinning weight adaptation.

use rig_assets::{AssetStore, SkinAsset, SkinAssetHandle, SkinWeights, SkinWeightsHandle};
use rig_scene::{NodeId, SceneGraph};

use crate::buffers;
use crate::error::{GltfError, Result};

/// Adapt all glTF skins into `SkinAsset` values registered in `store`.
pub(crate) fn adapt_skins(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    node_map: &[Option<NodeId>],
    scene: &SceneGraph,
    store: &mut AssetStore,
) -> Result<Vec<SkinAssetHandle>> {
    let mut handles = Vec::new();
    for skin in document.skins() {
        let joint_names = skin
            .joints()
            .map(|joint| {
                node_map
                    .get(joint.index())
                    .copied()
                    .flatten()
                    .and_then(|id| scene.node_name(id).ok().map(ToOwned::to_owned))
                    .unwrap_or_else(|| format!("joint_{}", joint.index()))
            })
            .collect();
        let inverse_bind_matrices = buffers::read_inverse_bind_matrices(&skin, buffers);
        handles.push(store.add_skin(SkinAsset {
            joint_names,
            inverse_bind_matrices,
        }));
    }
    Ok(handles)
}

/// Adapt skinning weights for a single glTF primitive.
pub(crate) fn adapt_skin_weights(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    store: &mut AssetStore,
) -> Result<Option<SkinWeightsHandle>> {
    let Some(joints_0) = buffers::read_joints(primitive, buffers, 0) else {
        return Ok(None);
    };
    let Some(weights_0) = buffers::read_weights(primitive, buffers, 0) else {
        return Err(GltfError::IncompleteSkinWeights { set: 0 });
    };

    let joints_1 = buffers::read_joints(primitive, buffers, 1).unwrap_or_default();
    let weights_1 = buffers::read_weights(primitive, buffers, 1).unwrap_or_default();

    Ok(Some(store.add_skin_weights(SkinWeights::from_gltf_sets(
        &joints_0, &weights_0, &joints_1, &weights_1,
    ))))
}
