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
            gltf::camera::Projection::Orthographic(orthographic) => Projection::Orthographic {
                left: -orthographic.xmag(),
                right: orthographic.xmag(),
                bottom: -orthographic.ymag(),
                top: orthographic.ymag(),
                near: orthographic.znear(),
                far: orthographic.zfar(),
            },
        };

        let _ = scene.set_camera(node_id, CameraComponent { projection });
    }
}

#[cfg(test)]
mod tests {
    use rig_scene::SceneGraph;

    use super::*;

    #[test]
    fn adapt_cameras_maps_orthographic_projection() {
        let gltf = gltf::Gltf::from_slice(
            br#"{
                "asset": { "version": "2.0" },
                "cameras": [
                    {
                        "type": "orthographic",
                        "orthographic": {
                            "xmag": 2.0,
                            "ymag": 1.0,
                            "znear": 0.1,
                            "zfar": 50.0
                        }
                    }
                ],
                "nodes": [ { "camera": 0 } ],
                "scenes": [ { "nodes": [0] } ],
                "scene": 0
            }"#,
        )
        .expect("valid orthographic camera glTF");
        let mut scene = SceneGraph::new();
        let node = scene.create_node("camera");
        let node_map = vec![Some(node)];

        adapt_cameras(&gltf.document, &node_map, &mut scene);

        let camera = scene.camera(node).expect("valid camera node");
        assert_eq!(
            camera.map(|component| component.projection),
            Some(Projection::Orthographic {
                left: -2.0,
                right: 2.0,
                bottom: -1.0,
                top: 1.0,
                near: 0.1,
                far: 50.0,
            })
        );
    }
}
