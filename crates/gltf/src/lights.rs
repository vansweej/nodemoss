//! KHR_lights_punctual → LightComponent adaptation.

use rig_math::Vec3;
use rig_scene::{LightComponent, LightKind, NodeId, SceneGraph};

/// Attach light components to scene nodes that have KHR_lights_punctual lights.
pub(crate) fn adapt_lights(
    document: &gltf::Document,
    node_map: &[Option<NodeId>],
    scene: &mut SceneGraph,
) {
    for node in document.nodes() {
        let Some(light) = node.light() else {
            continue;
        };
        let Some(node_id) = node_map.get(node.index()).copied().flatten() else {
            continue;
        };

        let color = light.color();
        let color = Vec3::new(color[0], color[1], color[2]);
        let kind = match light.kind() {
            gltf::khr_lights_punctual::Kind::Directional => LightKind::Directional {
                color,
                intensity: light.intensity(),
            },
            gltf::khr_lights_punctual::Kind::Point => LightKind::Point {
                color,
                intensity: light.intensity(),
                range: light.range().unwrap_or(20.0),
            },
            gltf::khr_lights_punctual::Kind::Spot { .. } => {
                log::warn!("glTF spot lights are not supported, skipping");
                continue;
            }
        };

        let _ = scene.set_light(node_id, LightComponent { kind });
    }
}
