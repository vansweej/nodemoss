//! glTF camera → CameraComponent adaptation.

use rig_math::Projection;
use rig_scene::{CameraComponent, NodeId, SceneGraph};

/// Attach camera components to scene nodes that have glTF cameras.
pub(crate) fn adapt_cameras(
    document: &gltf::Document,
    node_map: &[Option<NodeId>],
    scene: &mut SceneGraph,
) {
    for node in document.nodes() {
        let Some(camera) = node.camera() else {
            continue;
        };
        let Some(node_id) = node_map.get(node.index()).copied().flatten() else {
            continue;
        };

        let projection = match camera.projection() {
            gltf::camera::Projection::Perspective(perspective) => Projection::Perspective {
                fov_y_radians: perspective.yfov(),
                near: perspective.znear(),
                far: perspective.zfar().unwrap_or(1000.0),
            },
            gltf::camera::Projection::Orthographic(_) => {
                log::warn!("glTF orthographic cameras are not supported, skipping");
                continue;
            }
        };

        let _ = scene.set_camera(node_id, CameraComponent { projection });
    }
}
