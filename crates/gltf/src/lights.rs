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
            gltf::khr_lights_punctual::Kind::Spot {
                inner_cone_angle,
                outer_cone_angle,
            } => LightKind::Spot {
                color,
                intensity: light.intensity(),
                range: light.range().unwrap_or(20.0),
                inner_cone_angle,
                outer_cone_angle,
            },
        };

        let _ = scene.set_light(node_id, LightComponent { kind });
    }
}

#[cfg(test)]
mod tests {
    use rig_scene::SceneGraph;

    use super::*;

    #[test]
    fn adapt_lights_maps_spot_light() {
        let gltf = gltf::Gltf::from_slice(
            br#"{
                "asset": { "version": "2.0" },
                "extensionsUsed": ["KHR_lights_punctual"],
                "extensions": {
                    "KHR_lights_punctual": {
                        "lights": [
                            {
                                "type": "spot",
                                "color": [1.0, 0.5, 0.25],
                                "intensity": 3.0,
                                "range": 12.0,
                                "spot": {
                                    "innerConeAngle": 0.2,
                                    "outerConeAngle": 0.6
                                }
                            }
                        ]
                    }
                },
                "nodes": [
                    { "extensions": { "KHR_lights_punctual": { "light": 0 } } }
                ],
                "scenes": [ { "nodes": [0] } ],
                "scene": 0
            }"#,
        )
        .expect("valid spot light glTF");
        let mut scene = SceneGraph::new();
        let node = scene.create_node("spot");
        let node_map = vec![Some(node)];

        adapt_lights(&gltf.document, &node_map, &mut scene);

        assert_eq!(
            scene
                .light(node)
                .expect("valid node")
                .map(|light| light.kind),
            Some(LightKind::Spot {
                color: Vec3::new(1.0, 0.5, 0.25),
                intensity: 3.0,
                range: 12.0,
                inner_cone_angle: 0.2,
                outer_cone_angle: 0.6,
            })
        );
    }
}
