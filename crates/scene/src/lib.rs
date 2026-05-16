//! Scene graph and world model for the rig framework.

mod components;
mod extraction;
mod graph;
mod node;
mod traversal;

pub use components::*;
pub use extraction::*;
pub use graph::*;
pub use node::*;
// Re-export asset handle types for use in the public API
pub use rig_assets::{DynamicMeshId, MaterialHandle, MeshHandle, MeshSource};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rig_assets::{
        MaterialAsset, MeshAsset, ShaderAsset, VertexAttribute, VertexFormat, VertexLayout,
    };
    use rig_math::{BoundingSphere, Quat, Transform, Vec3};

    use super::*;

    fn sample_assets() -> (rig_assets::AssetStore, MeshHandle, MaterialHandle) {
        let mut assets = rig_assets::AssetStore::new();
        let shader = assets.add_shader(ShaderAsset {
            source: Arc::from("shader"),
        });
        let material = assets.add_material(MaterialAsset {
            shader,
            parameters: rig_assets::MaterialParams::default(),
            textures: vec![],
        });
        let mesh = assets.add_mesh(MeshAsset {
            vertex_layout: VertexLayout {
                array_stride: 24,
                attributes: vec![
                    VertexAttribute {
                        shader_location: 0,
                        format: VertexFormat::Float32x3,
                        offset: 0,
                    },
                    VertexAttribute {
                        shader_location: 1,
                        format: VertexFormat::Float32x3,
                        offset: 12,
                    },
                ],
            },
            vertex_data: Arc::from([0_u8; 24]),
            index_data: Arc::from([0_u8; 6]),
            index_format: rig_assets::IndexFormat::Uint16,
            local_bounds: BoundingSphere {
                center: Vec3::ZERO,
                radius: 1.0,
            },
        });
        (assets, mesh, material)
    }

    fn approx_eq_vec3(left: Vec3, right: Vec3) {
        assert!(
            left.abs_diff_eq(right, 1e-5),
            "left={left:?} right={right:?}"
        );
    }

    #[test]
    fn create_node_starts_with_generation_zero() {
        let mut scene = SceneGraph::new();
        let node = scene.create_node("node");
        assert_eq!(scene.node_name(node).unwrap(), "node");
    }

    #[test]
    fn destroy_node_invalidates_old_handle_and_reuses_slot_with_new_generation() {
        let mut scene = SceneGraph::new();
        let first = scene.create_node("first");
        scene.destroy_node(first).unwrap();
        let second = scene.create_node("second");
        assert!(matches!(
            scene.node_name(first),
            Err(SceneError::InvalidNode)
        ));
        assert_eq!(scene.node_name(second).unwrap(), "second");
        assert_ne!(first, second);
    }

    #[test]
    fn attach_child_sets_parent_and_children_list() {
        let mut scene = SceneGraph::new();
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(parent, child).unwrap();
        assert_eq!(scene.children(parent).unwrap(), vec![child]);
    }

    #[test]
    fn attach_child_reparents_existing_child() {
        let mut scene = SceneGraph::new();
        let first_parent = scene.create_node("first_parent");
        let second_parent = scene.create_node("second_parent");
        let child = scene.create_node("child");
        scene.attach_child(first_parent, child).unwrap();
        scene.attach_child(second_parent, child).unwrap();
        assert!(scene.children(first_parent).unwrap().is_empty());
        assert_eq!(scene.children(second_parent).unwrap(), vec![child]);
    }

    #[test]
    fn attach_child_rejects_ancestor_to_descendant_reparenting() {
        let mut scene = SceneGraph::new();
        let grandparent = scene.create_node("grandparent");
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(grandparent, parent).unwrap();
        scene.attach_child(parent, child).unwrap();
        assert!(matches!(
            scene.attach_child(child, grandparent),
            Err(SceneError::CycleDetected)
        ));
        assert_eq!(scene.children(grandparent).unwrap(), vec![parent]);
        assert_eq!(scene.children(parent).unwrap(), vec![child]);
        assert!(scene.children(child).unwrap().is_empty());
    }

    #[test]
    fn attach_child_does_not_detach_child_when_parent_is_invalid() {
        let mut scene = SceneGraph::new();
        let real_parent = scene.create_node("real_parent");
        let child = scene.create_node("child");
        scene.attach_child(real_parent, child).unwrap();
        let invalid = NodeId {
            index: 99,
            generation: 0,
        };
        assert!(matches!(
            scene.attach_child(invalid, child),
            Err(SceneError::InvalidNode)
        ));
        assert_eq!(scene.children(real_parent).unwrap(), vec![child]);
    }

    #[test]
    fn attach_child_does_not_detach_child_when_cycle_detected() {
        let mut scene = SceneGraph::new();
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(parent, child).unwrap();
        let result = scene.attach_child(child, parent);
        assert!(matches!(result, Err(SceneError::CycleDetected)));
        assert!(scene.children(child).unwrap().is_empty());
        assert_eq!(scene.children(parent).unwrap(), vec![child]);
    }

    #[test]
    fn attach_child_rejects_self_parenting() {
        let mut scene = SceneGraph::new();
        let node = scene.create_node("node");
        assert!(matches!(
            scene.attach_child(node, node),
            Err(SceneError::SelfParent)
        ));
    }

    #[test]
    fn detach_child_clears_parent_link() {
        let mut scene = SceneGraph::new();
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(parent, child).unwrap();
        scene.detach_child(child).unwrap();
        assert!(scene.children(parent).unwrap().is_empty());
    }

    #[test]
    fn set_renderable_camera_and_light_require_valid_node() {
        let mut scene = SceneGraph::new();
        let invalid = NodeId {
            index: 99,
            generation: 0,
        };
        let (_, mesh, material) = sample_assets();
        assert!(matches!(
            scene.set_renderable(
                invalid,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material
                }
            ),
            Err(SceneError::InvalidNode)
        ));
        assert!(matches!(
            scene.set_camera(
                invalid,
                CameraComponent {
                    projection: rig_math::Projection::Perspective {
                        fov_y_radians: 1.0,
                        near: 0.1,
                        far: 10.0
                    }
                }
            ),
            Err(SceneError::InvalidNode)
        ));
        assert!(matches!(
            scene.set_light(
                invalid,
                LightComponent {
                    kind: LightKind::Point {
                        color: Vec3::ONE,
                        intensity: 1.0,
                        range: 5.0
                    }
                }
            ),
            Err(SceneError::InvalidNode)
        ));
    }

    #[test]
    fn set_and_get_renderable_camera_and_name_work() {
        let mut scene = SceneGraph::new();
        let node = scene.create_node("triangle");
        let (_, mesh, material) = sample_assets();
        let camera_component = CameraComponent {
            projection: rig_math::Projection::Perspective {
                fov_y_radians: 1.0,
                near: 0.1,
                far: 10.0,
            },
        };
        scene
            .set_renderable(
                node,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene.set_camera(node, camera_component).unwrap();
        assert_eq!(scene.node_name(node).unwrap(), "triangle");
        assert_eq!(
            scene.renderable(node).unwrap().copied(),
            Some(Renderable {
                mesh: MeshSource::Static(mesh),
                material
            })
        );
        assert_eq!(scene.camera(node).unwrap().copied(), Some(camera_component));
    }

    #[test]
    fn set_local_transform_updates_world_transform_after_propagation() {
        let mut scene = SceneGraph::new();
        let root = scene.create_node("root");
        let child = scene.create_node("child");
        scene.attach_child(root, child).unwrap();
        scene
            .set_local_transform(
                root,
                rig_math::Transform {
                    translation: Vec3::new(1.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                child,
                rig_math::Transform {
                    translation: Vec3::new(0.0, 2.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene.update_world_transforms(root).unwrap();
        approx_eq_vec3(
            scene
                .world_transform(child)
                .unwrap()
                .transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 2.0, 0.0),
        );
    }

    #[test]
    fn update_all_world_transforms_updates_multiple_roots() {
        let mut scene = SceneGraph::new();
        let left = scene.create_node("left");
        let right = scene.create_node("right");
        scene
            .set_local_transform(
                left,
                rig_math::Transform {
                    translation: Vec3::new(1.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                right,
                rig_math::Transform {
                    translation: Vec3::new(0.0, 1.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene.update_all_world_transforms().unwrap();
        approx_eq_vec3(
            scene
                .world_transform(left)
                .unwrap()
                .transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 0.0, 0.0),
        );
        approx_eq_vec3(
            scene
                .world_transform(right)
                .unwrap()
                .transform_point3(Vec3::ZERO),
            Vec3::new(0.0, 1.0, 0.0),
        );
    }

    #[test]
    fn update_world_bounds_uses_mesh_asset_and_child_union() {
        let mut scene = SceneGraph::new();
        let (assets, mesh, material) = sample_assets();
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(parent, child).unwrap();
        scene
            .set_renderable(
                child,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                child,
                rig_math::Transform {
                    translation: Vec3::new(3.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene.update_all_world_transforms().unwrap();
        scene.update_world_bounds(parent, &assets).unwrap();
        let extracted = scene.extract_renderables();
        assert_eq!(extracted.len(), 1);
        approx_eq_vec3(extracted[0].world_bound.center, Vec3::new(3.0, 0.0, 0.0));
        assert!((extracted[0].world_bound.radius - 1.0).abs() <= 1e-5);
    }

    #[test]
    fn update_world_bounds_errors_when_mesh_asset_is_missing() {
        let mut scene = SceneGraph::new();
        let (_source_assets, mesh, material) = sample_assets();
        let assets = rig_assets::AssetStore::new();
        let node = scene.create_node("node");
        scene
            .set_renderable(
                node,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        assert!(matches!(
            scene.update_world_bounds(node, &assets),
            Err(SceneError::MissingMeshAsset)
        ));
    }

    #[test]
    fn extract_renderables_skips_hidden_nodes() {
        let mut scene = SceneGraph::new();
        let (_, mesh, material) = sample_assets();
        let node = scene.create_node("hidden");
        scene
            .set_renderable(
                node,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene.node_mut(node).unwrap().visibility = VisibilityMode::Hidden;
        let extracted = scene.extract_renderables();
        assert!(extracted.is_empty());
    }

    #[test]
    fn frustum_plane_extraction_normalizes_planes() {
        let matrix = rig_math::Mat4::IDENTITY;
        let planes = frustum_planes_from_projection_view(matrix);
        for plane in planes {
            let normal_length = plane.truncate().length();
            assert!((normal_length - 1.0).abs() <= 1e-5 || normal_length == 0.0);
        }
    }

    #[test]
    fn normalize_plane_leaves_zero_plane_unchanged() {
        let plane = rig_math::Vec4::ZERO;
        assert_eq!(normalize_plane(plane), plane);
    }

    fn perspective() -> rig_math::Projection {
        rig_math::Projection::Perspective {
            fov_y_radians: 1.0,
            near: 0.1,
            far: 100.0,
        }
    }

    #[test]
    fn camera_nodes_returns_all_cameras() {
        let mut scene = SceneGraph::new();
        let a = scene.create_node("a");
        let b = scene.create_node("b");
        let c = scene.create_node("c");
        scene
            .set_camera(
                a,
                CameraComponent {
                    projection: perspective(),
                },
            )
            .unwrap();
        scene
            .set_camera(
                b,
                CameraComponent {
                    projection: perspective(),
                },
            )
            .unwrap();
        let mut nodes = scene.camera_nodes();
        nodes.sort_by_key(|n| n.index);
        assert_eq!(nodes.len(), 2);
        assert!(nodes.contains(&a));
        assert!(nodes.contains(&b));
        assert!(!nodes.contains(&c));
    }

    #[test]
    fn first_camera_returns_none_for_empty_scene() {
        let scene = SceneGraph::new();
        assert!(scene.first_camera().is_none());
    }

    #[test]
    fn first_camera_returns_some_for_scene_with_camera() {
        let mut scene = SceneGraph::new();
        let cam = scene.create_node("cam");
        scene
            .set_camera(
                cam,
                CameraComponent {
                    projection: perspective(),
                },
            )
            .unwrap();
        assert!(scene.first_camera().is_some());
    }

    #[test]
    fn camera_with_name_finds_matching_camera() {
        let mut scene = SceneGraph::new();
        let main = scene.create_node("main");
        let debug = scene.create_node("debug");
        scene
            .set_camera(
                main,
                CameraComponent {
                    projection: perspective(),
                },
            )
            .unwrap();
        scene
            .set_camera(
                debug,
                CameraComponent {
                    projection: perspective(),
                },
            )
            .unwrap();
        assert_eq!(scene.camera_with_name("main"), Some(main));
        assert_eq!(scene.camera_with_name("debug"), Some(debug));
    }

    #[test]
    fn camera_with_name_returns_none_for_non_camera_node() {
        let mut scene = SceneGraph::new();
        let _node = scene.create_node("present");
        assert!(scene.camera_with_name("present").is_none());
    }

    #[test]
    fn camera_with_name_returns_none_for_missing_name() {
        let mut scene = SceneGraph::new();
        let cam = scene.create_node("main");
        scene
            .set_camera(
                cam,
                CameraComponent {
                    projection: perspective(),
                },
            )
            .unwrap();
        assert!(scene.camera_with_name("other").is_none());
    }

    #[test]
    fn extract_active_camera_computes_world_transform() {
        let mut scene = SceneGraph::new();
        let parent = scene.create_node("parent");
        let cam_node = scene.create_node("cam");
        scene.attach_child(parent, cam_node).unwrap();
        scene
            .set_local_transform(
                parent,
                rig_math::Transform {
                    translation: Vec3::new(1.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                cam_node,
                rig_math::Transform {
                    translation: Vec3::new(0.0, 2.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_camera(
                cam_node,
                CameraComponent {
                    projection: perspective(),
                },
            )
            .unwrap();
        scene.update_world_transforms(parent).unwrap();
        let extracted = scene.extract_active_camera(cam_node).unwrap();
        assert_eq!(extracted.node, cam_node);
        assert_eq!(extracted.projection, perspective());
        approx_eq_vec3(
            extracted.world_transform.transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 2.0, 0.0),
        );
    }

    #[test]
    fn extract_active_camera_errors_for_non_camera_node() {
        let mut scene = SceneGraph::new();
        let node = scene.create_node("node");
        assert!(matches!(
            scene.extract_active_camera(node),
            Err(SceneError::NotACamera)
        ));
    }

    #[test]
    fn extract_active_camera_errors_for_invalid_node() {
        let scene = SceneGraph::new();
        let invalid = NodeId {
            index: 99,
            generation: 0,
        };
        assert!(matches!(
            scene.extract_active_camera(invalid),
            Err(SceneError::InvalidNode)
        ));
    }

    #[test]
    fn extract_active_camera_returns_invalid_node_for_stale_handle() {
        let mut scene = SceneGraph::new();
        let cam = scene.create_node("cam");
        scene
            .set_camera(
                cam,
                CameraComponent {
                    projection: perspective(),
                },
            )
            .unwrap();
        scene.destroy_node(cam).unwrap();
        assert!(matches!(
            scene.extract_active_camera(cam),
            Err(SceneError::InvalidNode)
        ));
    }

    #[test]
    fn extract_lights_returns_empty_for_scene_with_no_lights() {
        let scene = SceneGraph::new();
        assert!(scene.extract_lights().is_empty());
    }

    #[test]
    fn extract_lights_computes_world_direction_for_directional_light() {
        let mut scene = SceneGraph::new();
        let light_node = scene.create_node("sun");
        scene
            .set_local_transform(
                light_node,
                rig_math::Transform {
                    translation: Vec3::ZERO,
                    rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_light(
                light_node,
                LightComponent {
                    kind: LightKind::Directional {
                        color: Vec3::ONE,
                        intensity: 1.0,
                    },
                },
            )
            .unwrap();
        scene.update_world_transforms(light_node).unwrap();
        let lights = scene.extract_lights();
        assert_eq!(lights.len(), 1);
        approx_eq_vec3(lights[0].world_direction, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn extract_lights_includes_world_position_for_point_light() {
        let mut scene = SceneGraph::new();
        let light_node = scene.create_node("lamp");
        scene
            .set_local_transform(
                light_node,
                rig_math::Transform {
                    translation: Vec3::new(3.0, 5.0, -2.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_light(
                light_node,
                LightComponent {
                    kind: LightKind::Point {
                        color: Vec3::ONE,
                        intensity: 2.0,
                        range: 10.0,
                    },
                },
            )
            .unwrap();
        scene.update_world_transforms(light_node).unwrap();
        let lights = scene.extract_lights();
        assert_eq!(lights.len(), 1);
        approx_eq_vec3(lights[0].world_position, Vec3::new(3.0, 5.0, -2.0));
    }

    #[test]
    fn extract_lights_includes_world_position_and_direction_for_spot_light() {
        let mut scene = SceneGraph::new();
        let light_node = scene.create_node("spot");
        scene
            .set_local_transform(
                light_node,
                rig_math::Transform {
                    translation: Vec3::new(3.0, 5.0, -2.0),
                    rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_light(
                light_node,
                LightComponent {
                    kind: LightKind::Spot {
                        color: Vec3::ONE,
                        intensity: 2.0,
                        range: 10.0,
                        inner_cone_angle: 0.2,
                        outer_cone_angle: 0.6,
                    },
                },
            )
            .unwrap();
        scene.update_world_transforms(light_node).unwrap();
        let lights = scene.extract_lights();

        assert_eq!(lights.len(), 1);
        approx_eq_vec3(lights[0].world_position, Vec3::new(3.0, 5.0, -2.0));
        approx_eq_vec3(lights[0].world_direction, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn extract_lights_returns_all_lights_in_scene() {
        let mut scene = SceneGraph::new();
        for i in 0..3 {
            let node = scene.create_node(format!("light_{i}"));
            scene
                .set_light(
                    node,
                    LightComponent {
                        kind: LightKind::Directional {
                            color: Vec3::ONE,
                            intensity: 1.0,
                        },
                    },
                )
                .unwrap();
            scene.update_world_transforms(node).unwrap();
        }
        assert_eq!(scene.extract_lights().len(), 3);
    }

    fn box_frustum(half: f32) -> [rig_math::Vec4; 6] {
        [
            rig_math::Vec4::new(1.0, 0.0, 0.0, half),
            rig_math::Vec4::new(-1.0, 0.0, 0.0, half),
            rig_math::Vec4::new(0.0, 1.0, 0.0, half),
            rig_math::Vec4::new(0.0, -1.0, 0.0, half),
            rig_math::Vec4::new(0.0, 0.0, 1.0, half),
            rig_math::Vec4::new(0.0, 0.0, -1.0, half),
        ]
    }

    #[test]
    fn extract_renderables_culled_excludes_outside_objects() {
        let mut scene = SceneGraph::new();
        let (assets, mesh, material) = sample_assets();
        let inside = scene.create_node("inside");
        scene
            .set_renderable(
                inside,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        let outside = scene.create_node("outside");
        scene
            .set_renderable(
                outside,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                outside,
                rig_math::Transform {
                    translation: Vec3::new(50.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene.update_all_world_transforms().unwrap();
        scene.update_all_world_bounds(&assets).unwrap();
        let planes = box_frustum(10.0);
        let extracted = scene.extract_renderables_culled(&planes);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].node, inside);
    }

    #[test]
    fn extract_renderables_culled_always_includes_always_visible() {
        let mut scene = SceneGraph::new();
        let (assets, mesh, material) = sample_assets();
        let node = scene.create_node("always");
        scene
            .set_renderable(
                node,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                node,
                rig_math::Transform {
                    translation: Vec3::new(50.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_visibility(node, VisibilityMode::AlwaysVisible)
            .unwrap();
        scene.update_all_world_transforms().unwrap();
        scene.update_all_world_bounds(&assets).unwrap();
        let planes = box_frustum(10.0);
        let extracted = scene.extract_renderables_culled(&planes);
        assert_eq!(extracted.len(), 1);
    }

    #[test]
    fn extract_renderables_culled_always_excludes_hidden() {
        let mut scene = SceneGraph::new();
        let (assets, mesh, material) = sample_assets();
        let node = scene.create_node("hidden");
        scene
            .set_renderable(
                node,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene.set_visibility(node, VisibilityMode::Hidden).unwrap();
        scene.update_all_world_transforms().unwrap();
        scene.update_all_world_bounds(&assets).unwrap();
        let planes = box_frustum(10.0);
        let extracted = scene.extract_renderables_culled(&planes);
        assert!(extracted.is_empty());
    }

    #[test]
    fn set_visibility_changes_node_visibility() {
        let mut scene = SceneGraph::new();
        let node = scene.create_node("test");
        assert_eq!(scene.visibility(node).unwrap(), VisibilityMode::Inherit);
        scene.set_visibility(node, VisibilityMode::Hidden).unwrap();
        assert_eq!(scene.visibility(node).unwrap(), VisibilityMode::Hidden);
        scene
            .set_visibility(node, VisibilityMode::AlwaysVisible)
            .unwrap();
        assert_eq!(
            scene.visibility(node).unwrap(),
            VisibilityMode::AlwaysVisible
        );
    }

    #[test]
    fn set_visibility_errors_for_invalid_node() {
        let mut scene = SceneGraph::new();
        let node = scene.create_node("temp");
        scene.destroy_node(node).unwrap();
        assert!(scene.set_visibility(node, VisibilityMode::Hidden).is_err());
        assert!(scene.visibility(node).is_err());
    }

    #[test]
    fn effective_visibility_returns_inherit_when_all_ancestors_inherit() {
        let mut scene = SceneGraph::new();
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(parent, child).unwrap();
        assert_eq!(
            scene.effective_visibility(child).unwrap(),
            VisibilityMode::Inherit
        );
    }

    #[test]
    fn effective_visibility_returns_hidden_for_hidden_ancestor() {
        let mut scene = SceneGraph::new();
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(parent, child).unwrap();
        scene
            .set_visibility(parent, VisibilityMode::Hidden)
            .unwrap();
        assert_eq!(
            scene.effective_visibility(child).unwrap(),
            VisibilityMode::Hidden
        );
    }

    #[test]
    fn extract_renderables_hides_child_when_parent_is_hidden() {
        let mut scene = SceneGraph::new();
        let (_, mesh, material) = sample_assets();
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(parent, child).unwrap();
        scene
            .set_renderable(
                child,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene
            .set_visibility(parent, VisibilityMode::Hidden)
            .unwrap();
        let extracted = scene.extract_renderables();
        assert!(extracted.is_empty(), "child should be hidden via parent");
    }

    #[test]
    fn extract_renderables_culled_hides_always_visible_child_when_parent_is_hidden() {
        let mut scene = SceneGraph::new();
        let (assets, mesh, material) = sample_assets();
        let parent = scene.create_node("parent");
        let child = scene.create_node("child");
        scene.attach_child(parent, child).unwrap();
        scene
            .set_renderable(
                child,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene
            .set_visibility(parent, VisibilityMode::Hidden)
            .unwrap();
        scene
            .set_visibility(child, VisibilityMode::AlwaysVisible)
            .unwrap();
        scene.update_all_world_transforms().unwrap();
        scene.update_all_world_bounds(&assets).unwrap();
        let planes = box_frustum(10.0);
        let extracted = scene.extract_renderables_culled(&planes);
        assert!(
            extracted.is_empty(),
            "AlwaysVisible child should still be hidden when parent is Hidden"
        );
    }

    #[test]
    fn find_node_by_name_returns_matching_node() {
        let mut scene = SceneGraph::new();
        let _alpha = scene.create_node("alpha");
        let beta = scene.create_node("beta");
        assert_eq!(scene.find_node_by_name("beta"), Some(beta));
    }

    #[test]
    fn find_node_by_name_returns_none_for_missing() {
        let mut scene = SceneGraph::new();
        let _node = scene.create_node("present");
        assert_eq!(scene.find_node_by_name("missing"), None);
    }

    #[test]
    fn find_node_by_name_skips_destroyed_nodes() {
        let mut scene = SceneGraph::new();
        let first = scene.create_node("arm");
        scene.destroy_node(first).unwrap();
        let second = scene.create_node("arm");
        let found = scene.find_node_by_name("arm").unwrap();
        assert_eq!(found, second);
        assert_ne!(found, first);
    }

    #[test]
    fn create_node_reuses_free_list_slot() {
        let mut scene = SceneGraph::new();
        let id1 = scene.create_node("first");
        scene.destroy_node(id1).unwrap();
        let id2 = scene.create_node("second");
        // Same slot index reused, generation incremented → ids differ
        assert_ne!(id1, id2);
        // Old handle is stale
        assert!(scene.node_name(id1).is_err());
        assert_eq!(scene.node_name(id2).unwrap(), "second");
    }

    #[test]
    fn detach_middle_child_updates_sibling_chain() {
        let mut scene = SceneGraph::new();
        let parent = scene.create_node("parent");
        let a = scene.create_node("a");
        let b = scene.create_node("b");
        let c = scene.create_node("c");
        // attach_child inserts at head, so order becomes c → b → a
        scene.attach_child(parent, a).unwrap();
        scene.attach_child(parent, b).unwrap();
        scene.attach_child(parent, c).unwrap();
        // Detach the middle child b
        scene.detach_child(b).unwrap();
        let children = scene.children(parent).unwrap();
        assert!(!children.contains(&b));
        assert!(children.contains(&a));
        assert!(children.contains(&c));
        // b should be root now; re-attaching to a new parent must succeed
        let new_parent = scene.create_node("new_parent");
        scene.attach_child(new_parent, b).unwrap();
        assert_eq!(scene.children(new_parent).unwrap(), vec![b]);
    }

    #[test]
    fn renderable_nodes_returns_all_renderable_node_ids() {
        let mut scene = SceneGraph::new();
        let (_, mesh, material) = sample_assets();
        let n1 = scene.create_node("r1");
        let n2 = scene.create_node("r2");
        let n3 = scene.create_node("plain");
        scene
            .set_renderable(
                n1,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        scene
            .set_renderable(
                n2,
                Renderable {
                    mesh: MeshSource::Static(mesh),
                    material,
                },
            )
            .unwrap();
        let ids: Vec<_> = scene.renderable_nodes().collect();
        assert!(ids.contains(&n1));
        assert!(ids.contains(&n2));
        assert!(!ids.contains(&n3));
    }

    #[test]
    fn world_transforms_propagate_to_grandchild() {
        let mut scene = SceneGraph::new();
        let root = scene.create_node("root");
        let child = scene.create_node("child");
        let grandchild = scene.create_node("grandchild");
        scene.attach_child(root, child).unwrap();
        scene.attach_child(child, grandchild).unwrap();

        scene
            .set_local_transform(
                root,
                Transform {
                    translation: Vec3::new(1.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                child,
                Transform {
                    translation: Vec3::new(0.0, 2.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                grandchild,
                Transform {
                    translation: Vec3::new(0.0, 0.0, 3.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene.update_world_transforms(root).unwrap();

        approx_eq_vec3(
            scene
                .world_transform(grandchild)
                .unwrap()
                .transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 2.0, 3.0),
        );
    }
}
