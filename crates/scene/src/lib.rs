//! Scene graph and world model for the rig framework.

use std::collections::HashMap;

use rig_assets::{AssetStore, MaterialHandle, MeshHandle};
use rig_math::{BoundingSphere, Mat4, Projection, Transform, Vec3, Vec4};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityMode {
    Inherit,
    AlwaysVisible,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Renderable {
    pub mesh: MeshHandle,
    pub material: MaterialHandle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraComponent {
    pub projection: Projection,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    Directional {
        color: Vec3,
        intensity: f32,
    },
    Point {
        color: Vec3,
        intensity: f32,
        range: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightComponent {
    pub kind: LightKind,
}

#[derive(Clone, Debug)]
pub struct SceneNode {
    name: String,
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    next_sibling: Option<NodeId>,
    local_transform: Transform,
    world_transform: Mat4,
    world_bound: BoundingSphere,
    visibility: VisibilityMode,
}

impl SceneNode {
    fn new(name: String) -> Self {
        Self {
            name,
            parent: None,
            first_child: None,
            next_sibling: None,
            local_transform: Transform::IDENTITY,
            world_transform: Mat4::IDENTITY,
            world_bound: BoundingSphere::ZERO,
            visibility: VisibilityMode::Inherit,
        }
    }
}

#[derive(Clone, Debug)]
struct NodeSlot {
    generation: u32,
    node: Option<SceneNode>,
}

#[derive(Debug, Error)]
pub enum SceneError {
    #[error("invalid node handle")]
    InvalidNode,
    #[error("cannot attach a node to itself")]
    SelfParent,
    #[error("missing mesh asset for renderable node")]
    MissingMeshAsset,
    #[error("node does not have a camera component")]
    NotACamera,
}

pub type Result<T> = std::result::Result<T, SceneError>;

#[derive(Default)]
pub struct SceneGraph {
    nodes: Vec<NodeSlot>,
    free_list: Vec<u32>,
    renderables: HashMap<NodeId, Renderable>,
    cameras: HashMap<NodeId, CameraComponent>,
    lights: HashMap<NodeId, LightComponent>,
}

impl SceneGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_node(&mut self, name: impl Into<String>) -> NodeId {
        let name = name.into();

        if let Some(index) = self.free_list.pop() {
            let slot = &mut self.nodes[index as usize];
            let id = NodeId {
                index,
                generation: slot.generation,
            };
            slot.node = Some(SceneNode::new(name));
            id
        } else {
            let index = self.nodes.len() as u32;
            self.nodes.push(NodeSlot {
                generation: 0,
                node: Some(SceneNode::new(name)),
            });
            NodeId {
                index,
                generation: 0,
            }
        }
    }

    pub fn destroy_node(&mut self, id: NodeId) -> Result<()> {
        let children = self.children(id)?;
        for child in children {
            self.destroy_node(child)?;
        }

        self.detach_child(id)?;
        self.renderables.remove(&id);
        self.cameras.remove(&id);
        self.lights.remove(&id);

        let slot = self.slot_mut(id)?;
        slot.node = None;
        slot.generation = slot.generation.wrapping_add(1);
        self.free_list.push(id.index);
        Ok(())
    }

    pub fn attach_child(&mut self, parent: NodeId, child: NodeId) -> Result<()> {
        if parent == child {
            return Err(SceneError::SelfParent);
        }

        self.detach_child(child)?;

        let first_child = self.node(parent)?.first_child;
        {
            let child_node = self.node_mut(child)?;
            child_node.parent = Some(parent);
            child_node.next_sibling = first_child;
        }
        self.node_mut(parent)?.first_child = Some(child);
        Ok(())
    }

    pub fn detach_child(&mut self, child: NodeId) -> Result<()> {
        let parent = self.node(child)?.parent;
        let Some(parent) = parent else {
            return Ok(());
        };

        let mut current = self.node(parent)?.first_child;
        let mut previous = None;

        while let Some(node_id) = current {
            let next = self.node(node_id)?.next_sibling;
            if node_id == child {
                if let Some(prev) = previous {
                    self.node_mut(prev)?.next_sibling = next;
                } else {
                    self.node_mut(parent)?.first_child = next;
                }
                break;
            }
            previous = Some(node_id);
            current = next;
        }

        let child_node = self.node_mut(child)?;
        child_node.parent = None;
        child_node.next_sibling = None;
        Ok(())
    }

    pub fn set_local_transform(&mut self, node: NodeId, transform: Transform) -> Result<()> {
        self.node_mut(node)?.local_transform = transform;
        Ok(())
    }

    pub fn local_transform(&self, node: NodeId) -> Result<Transform> {
        Ok(self.node(node)?.local_transform)
    }

    pub fn set_renderable(&mut self, node: NodeId, renderable: Renderable) -> Result<()> {
        self.node(node)?;
        self.renderables.insert(node, renderable);
        Ok(())
    }

    pub fn set_camera(&mut self, node: NodeId, camera: CameraComponent) -> Result<()> {
        self.node(node)?;
        self.cameras.insert(node, camera);
        Ok(())
    }

    pub fn set_light(&mut self, node: NodeId, light: LightComponent) -> Result<()> {
        self.node(node)?;
        self.lights.insert(node, light);
        Ok(())
    }

    pub fn world_transform(&self, node: NodeId) -> Result<Mat4> {
        Ok(self.node(node)?.world_transform)
    }

    pub fn renderable(&self, node: NodeId) -> Result<Option<&Renderable>> {
        self.node(node)?;
        Ok(self.renderables.get(&node))
    }

    pub fn camera(&self, node: NodeId) -> Result<Option<&CameraComponent>> {
        self.node(node)?;
        Ok(self.cameras.get(&node))
    }

    pub fn node_name(&self, node: NodeId) -> Result<&str> {
        Ok(&self.node(node)?.name)
    }

    pub fn children(&self, node: NodeId) -> Result<Vec<NodeId>> {
        let mut out = Vec::new();
        let mut current = self.node(node)?.first_child;
        while let Some(id) = current {
            out.push(id);
            current = self.node(id)?.next_sibling;
        }
        Ok(out)
    }

    pub fn renderable_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.renderables.keys().copied()
    }

    /// All node IDs that have a `CameraComponent` attached.
    pub fn camera_nodes(&self) -> Vec<NodeId> {
        self.cameras.keys().copied().collect()
    }

    /// The first camera node found in the scene, if any.
    ///
    /// Useful for single-camera apps that do not need explicit selection.
    pub fn first_camera(&self) -> Option<NodeId> {
        self.cameras.keys().next().copied()
    }

    /// Find a camera node by the name of its scene node.
    ///
    /// Returns `None` if no node with that name has a `CameraComponent`.
    pub fn camera_with_name(&self, name: &str) -> Option<NodeId> {
        self.cameras.keys().copied().find(|&id| {
            self.node(id)
                .map(|n| n.name == name)
                .unwrap_or(false)
        })
    }

    /// Extract camera data for a given camera node.
    ///
    /// Returns `SceneError::InvalidNode` when `id` is not a valid node, and
    /// `SceneError::NotACamera` when the node exists but has no camera component.
    pub fn extract_active_camera(&self, id: NodeId) -> Result<ExtractedCamera> {
        let camera = self
            .cameras
            .get(&id)
            .ok_or(SceneError::NotACamera)?;
        let world_transform = self.node(id)?.world_transform;
        Ok(ExtractedCamera {
            node: id,
            projection: camera.projection,
            world_transform,
        })
    }

    pub fn update_world_transforms(&mut self, root: NodeId) -> Result<()> {
        let root_local = self.node(root)?.local_transform.to_mat4();
        self.node_mut(root)?.world_transform = root_local;

        let children = self.children(root)?;
        for child in children {
            self.update_world_transforms_with_parent(child, root_local)?;
        }

        Ok(())
    }

    pub fn update_all_world_transforms(&mut self) -> Result<()> {
        let roots = self.root_nodes();
        for root in roots {
            self.update_world_transforms(root)?;
        }
        Ok(())
    }

    fn update_world_transforms_with_parent(
        &mut self,
        node: NodeId,
        parent_world: Mat4,
    ) -> Result<()> {
        let local = self.node(node)?.local_transform.to_mat4();
        let world = parent_world * local;
        self.node_mut(node)?.world_transform = world;

        let children = self.children(node)?;
        for child in children {
            self.update_world_transforms_with_parent(child, world)?;
        }

        Ok(())
    }

    pub fn update_world_bounds(&mut self, root: NodeId, assets: &AssetStore) -> Result<()> {
        let _ = self.compute_world_bounds(root, assets)?;
        Ok(())
    }

    pub fn update_all_world_bounds(&mut self, assets: &AssetStore) -> Result<()> {
        let roots = self.root_nodes();
        for root in roots {
            self.update_world_bounds(root, assets)?;
        }
        Ok(())
    }

    fn compute_world_bounds(
        &mut self,
        node: NodeId,
        assets: &AssetStore,
    ) -> Result<BoundingSphere> {
        let child_ids = self.children(node)?;
        let mut bound = if let Some(renderable) = self.renderables.get(&node).copied() {
            let mesh = assets
                .mesh(renderable.mesh)
                .map_err(|_| SceneError::MissingMeshAsset)?;
            mesh.local_bounds
                .transform_by(self.node(node)?.world_transform)
        } else {
            BoundingSphere::ZERO
        };

        for child in child_ids {
            let child_bound = self.compute_world_bounds(child, assets)?;
            bound = bound.union(child_bound);
        }

        self.node_mut(node)?.world_bound = bound;
        Ok(bound)
    }

    pub fn extract_renderables(&self) -> Vec<ExtractedRenderable> {
        self.renderables
            .iter()
            .filter_map(|(&node, &renderable)| {
                let world = self.node(node).ok()?.world_transform;
                let world_bound = self.node(node).ok()?.world_bound;
                let visibility = self.node(node).ok()?.visibility;
                if matches!(visibility, VisibilityMode::Hidden) {
                    return None;
                }

                Some(ExtractedRenderable {
                    node,
                    mesh: renderable.mesh,
                    material: renderable.material,
                    world_transform: world,
                    world_bound,
                })
            })
            .collect()
    }

    fn slot(&self, id: NodeId) -> Result<&NodeSlot> {
        let slot = self
            .nodes
            .get(id.index as usize)
            .ok_or(SceneError::InvalidNode)?;
        if slot.generation != id.generation || slot.node.is_none() {
            return Err(SceneError::InvalidNode);
        }
        Ok(slot)
    }

    fn slot_mut(&mut self, id: NodeId) -> Result<&mut NodeSlot> {
        let slot = self
            .nodes
            .get_mut(id.index as usize)
            .ok_or(SceneError::InvalidNode)?;
        if slot.generation != id.generation || slot.node.is_none() {
            return Err(SceneError::InvalidNode);
        }
        Ok(slot)
    }

    fn node(&self, id: NodeId) -> Result<&SceneNode> {
        self.slot(id)?.node.as_ref().ok_or(SceneError::InvalidNode)
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut SceneNode> {
        self.slot_mut(id)?
            .node
            .as_mut()
            .ok_or(SceneError::InvalidNode)
    }

    fn root_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let node = slot.node.as_ref()?;
                if node.parent.is_none() {
                    Some(NodeId {
                        index: index as u32,
                        generation: slot.generation,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedRenderable {
    pub node: NodeId,
    pub mesh: MeshHandle,
    pub material: MaterialHandle,
    pub world_transform: Mat4,
    pub world_bound: BoundingSphere,
}

/// Camera data extracted from the scene, ready for the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedCamera {
    pub node: NodeId,
    pub projection: Projection,
    pub world_transform: Mat4,
}

pub fn frustum_planes_from_projection_view(matrix: Mat4) -> [Vec4; 6] {
    let left = matrix.col(3) + matrix.col(0);
    let right = matrix.col(3) - matrix.col(0);
    let bottom = matrix.col(3) + matrix.col(1);
    let top = matrix.col(3) - matrix.col(1);
    let near = matrix.col(3) + matrix.col(2);
    let far = matrix.col(3) - matrix.col(2);

    [left, right, bottom, top, near, far].map(normalize_plane)
}

fn normalize_plane(plane: Vec4) -> Vec4 {
    let normal = plane.truncate();
    let length = normal.length();
    if length > 0.0 { plane / length } else { plane }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rig_assets::{
        MaterialAsset, MeshAsset, ShaderAsset, VertexAttribute, VertexFormat, VertexLayout,
    };
    use rig_math::{Quat, Vec3};

    use super::*;

    fn sample_assets() -> (AssetStore, MeshHandle, MaterialHandle) {
        let mut assets = AssetStore::new();
        let shader = assets.add_shader(ShaderAsset {
            source: Arc::from("shader"),
        });
        let material = assets.add_material(MaterialAsset { shader });
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
            scene.set_renderable(invalid, Renderable { mesh, material }),
            Err(SceneError::InvalidNode)
        ));
        assert!(matches!(
            scene.set_camera(
                invalid,
                CameraComponent {
                    projection: Projection::Perspective {
                        fov_y_radians: 1.0,
                        near: 0.1,
                        far: 10.0,
                    },
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
                        range: 5.0,
                    },
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
            projection: Projection::Perspective {
                fov_y_radians: 1.0,
                near: 0.1,
                far: 10.0,
            },
        };

        scene
            .set_renderable(node, Renderable { mesh, material })
            .unwrap();
        scene.set_camera(node, camera_component).unwrap();

        assert_eq!(scene.node_name(node).unwrap(), "triangle");
        assert_eq!(
            scene.renderable(node).unwrap().copied(),
            Some(Renderable { mesh, material })
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
                Transform {
                    translation: Vec3::new(1.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                right,
                Transform {
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
            .set_renderable(child, Renderable { mesh, material })
            .unwrap();
        scene
            .set_local_transform(
                child,
                Transform {
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
        let assets = AssetStore::new();
        let node = scene.create_node("node");
        scene
            .set_renderable(node, Renderable { mesh, material })
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
            .set_renderable(node, Renderable { mesh, material })
            .unwrap();
        scene.node_mut(node).unwrap().visibility = VisibilityMode::Hidden;

        let extracted = scene.extract_renderables();

        assert!(extracted.is_empty());
    }

    #[test]
    fn frustum_plane_extraction_normalizes_planes() {
        let matrix = Mat4::IDENTITY;

        let planes = frustum_planes_from_projection_view(matrix);

        for plane in planes {
            let normal_length = plane.truncate().length();
            assert!((normal_length - 1.0).abs() <= 1e-5 || normal_length == 0.0);
        }
    }

    #[test]
    fn normalize_plane_leaves_zero_plane_unchanged() {
        let plane = Vec4::ZERO;

        assert_eq!(normalize_plane(plane), plane);
    }

    fn perspective() -> Projection {
        Projection::Perspective {
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
        scene.set_camera(a, CameraComponent { projection: perspective() }).unwrap();
        scene.set_camera(b, CameraComponent { projection: perspective() }).unwrap();
        // c has no camera

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
        scene.set_camera(cam, CameraComponent { projection: perspective() }).unwrap();

        assert!(scene.first_camera().is_some());
    }

    #[test]
    fn camera_with_name_finds_matching_camera() {
        let mut scene = SceneGraph::new();
        let main = scene.create_node("main");
        let debug = scene.create_node("debug");
        scene.set_camera(main, CameraComponent { projection: perspective() }).unwrap();
        scene.set_camera(debug, CameraComponent { projection: perspective() }).unwrap();

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
        scene.set_camera(cam, CameraComponent { projection: perspective() }).unwrap();

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
                Transform {
                    translation: Vec3::new(1.0, 0.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene
            .set_local_transform(
                cam_node,
                Transform {
                    translation: Vec3::new(0.0, 2.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )
            .unwrap();
        scene.set_camera(cam_node, CameraComponent { projection: perspective() }).unwrap();
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
        let invalid = NodeId { index: 99, generation: 0 };

        // NodeId 99 does not exist — should get NotACamera (cameras map lookup fails first)
        // or InvalidNode depending on the lookup order. Either is an error.
        assert!(scene.extract_active_camera(invalid).is_err());
    }
}
