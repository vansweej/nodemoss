//! glTF node tree → SceneGraph hierarchy.

use rig_math::{Quat, Transform, Vec3};
use rig_scene::{NodeId, SceneGraph};

/// Adapt the default glTF scene's node tree into `scene`.
pub(crate) fn adapt_nodes(
    document: &gltf::Document,
    scene: &mut SceneGraph,
) -> (Vec<Option<NodeId>>, Vec<NodeId>) {
    let mut node_map = vec![None; document.nodes().count()];
    let Some(gltf_scene) = document
        .default_scene()
        .or_else(|| document.scenes().next())
    else {
        return (node_map, Vec::new());
    };

    let roots = gltf_scene
        .nodes()
        .map(|node| create_node_recursive(node, None, &mut node_map, scene))
        .collect();

    (node_map, roots)
}

fn create_node_recursive(
    node: gltf::Node<'_>,
    parent: Option<NodeId>,
    node_map: &mut [Option<NodeId>],
    scene: &mut SceneGraph,
) -> NodeId {
    let name = node
        .name()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("node_{}", node.index()));
    let id = scene.create_node(name);
    node_map[node.index()] = Some(id);

    let (translation, rotation, scale) = node.transform().decomposed();
    let _ = scene.set_local_transform(
        id,
        Transform {
            translation: Vec3::new(translation[0], translation[1], translation[2]),
            rotation: Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]),
            scale: Vec3::new(scale[0], scale[1], scale[2]),
        },
    );

    if let Some(parent) = parent {
        let _ = scene.attach_child(parent, id);
    }

    for child in node.children() {
        create_node_recursive(child, Some(id), node_map, scene);
    }

    id
}
