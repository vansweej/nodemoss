//! Component maps: renderables, cameras, lights, transforms, visibility.

use rig_assets::{AssetStore, MaterialHandle, MeshSource};
use rig_math::{BoundingSphere, Mat4, Projection, Transform, Vec3, Vec4};

use crate::SceneGraph;
use crate::extraction::{ExtractedCamera, ExtractedLight, ExtractedRenderable};
use crate::node::{NodeId, Result, SceneError, VisibilityMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Renderable {
    pub mesh: MeshSource,
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

impl SceneGraph {
    pub fn set_renderable(&mut self, node: NodeId, renderable: Renderable) -> Result<()> {
        self.node(node)?;
        self.renderables.insert(node, renderable);
        Ok(())
    }

    pub fn renderable(&self, node: NodeId) -> Result<Option<&Renderable>> {
        self.node(node)?;
        Ok(self.renderables.get(&node))
    }

    pub fn renderable_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.renderables.keys().copied()
    }

    pub fn set_camera(&mut self, node: NodeId, camera: CameraComponent) -> Result<()> {
        self.node(node)?;
        self.cameras.insert(node, camera);
        Ok(())
    }

    pub fn camera(&self, node: NodeId) -> Result<Option<&CameraComponent>> {
        self.node(node)?;
        Ok(self.cameras.get(&node))
    }

    pub fn camera_nodes(&self) -> Vec<NodeId> {
        self.cameras.keys().copied().collect()
    }

    pub fn first_camera(&self) -> Option<NodeId> {
        self.cameras.keys().next().copied()
    }

    pub fn camera_with_name(&self, name: &str) -> Option<NodeId> {
        self.cameras
            .keys()
            .copied()
            .find(|&id| self.node(id).map(|n| n.name == name).unwrap_or(false))
    }

    pub fn set_light(&mut self, node: NodeId, light: LightComponent) -> Result<()> {
        self.node(node)?;
        self.lights.insert(node, light);
        Ok(())
    }

    pub fn light(&self, node: NodeId) -> Result<Option<&LightComponent>> {
        self.node(node)?;
        Ok(self.lights.get(&node))
    }

    pub fn light_nodes(&self) -> Vec<NodeId> {
        self.lights.keys().copied().collect()
    }

    pub fn set_local_transform(&mut self, node: NodeId, transform: Transform) -> Result<()> {
        self.node_mut(node)?.local_transform = transform;
        Ok(())
    }

    pub fn local_transform(&self, node: NodeId) -> Result<Transform> {
        Ok(self.node(node)?.local_transform)
    }

    pub fn world_transform(&self, node: NodeId) -> Result<Mat4> {
        Ok(self.node(node)?.world_transform)
    }

    pub fn world_bound(&self, node: NodeId) -> Result<BoundingSphere> {
        Ok(self.node(node)?.world_bound)
    }

    pub fn node_name(&self, node: NodeId) -> Result<&str> {
        Ok(&self.node(node)?.name)
    }

    /// Find the first live node whose name equals `name`.
    ///
    /// O(n) linear scan over all node slots — intended for bind-time use
    /// (called once when a clip is assigned), not per-frame.
    /// Returns `None` if no matching node exists.
    pub fn find_node_by_name(&self, name: &str) -> Option<NodeId> {
        self.nodes.iter().enumerate().find_map(|(index, slot)| {
            let node = slot.node.as_ref()?;
            if node.name == name {
                Some(NodeId {
                    index: index as u32,
                    generation: slot.generation,
                })
            } else {
                None
            }
        })
    }

    pub fn visibility(&self, id: NodeId) -> Result<VisibilityMode> {
        Ok(self.node(id)?.visibility)
    }

    pub fn set_visibility(&mut self, id: NodeId, mode: VisibilityMode) -> Result<()> {
        self.node_mut(id)?.visibility = mode;
        Ok(())
    }

    pub fn effective_visibility(&self, id: NodeId) -> Result<VisibilityMode> {
        let own = self.node(id)?.visibility;
        let mut current = self.node(id)?.parent;
        while let Some(parent_id) = current {
            let parent = self.node(parent_id)?;
            if parent.visibility == VisibilityMode::Hidden {
                return Ok(VisibilityMode::Hidden);
            }
            current = parent.parent;
        }
        Ok(own)
    }

    pub fn extract_active_camera(&self, id: NodeId) -> Result<ExtractedCamera> {
        let world_transform = self.node(id)?.world_transform;
        let camera = self.cameras.get(&id).ok_or(SceneError::NotACamera)?;
        Ok(ExtractedCamera {
            node: id,
            projection: camera.projection,
            world_transform,
        })
    }

    pub fn extract_renderables(&self) -> Vec<ExtractedRenderable> {
        self.renderables
            .iter()
            .filter_map(|(&node, &renderable)| {
                let world = self.node(node).ok()?.world_transform;
                let world_bound = self.node(node).ok()?.world_bound;
                let visibility = self.effective_visibility(node).ok()?;
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

    pub fn extract_lights(&self) -> Vec<ExtractedLight> {
        self.lights
            .iter()
            .filter_map(|(&node, &light)| {
                let world = self.node(node).ok()?.world_transform;
                let world_position = world.transform_point3(Vec3::ZERO);
                let world_direction = world.transform_vector3(-Vec3::Z).normalize_or_zero();
                Some(ExtractedLight {
                    kind: light.kind,
                    world_position,
                    world_direction,
                })
            })
            .collect()
    }

    pub fn extract_renderables_culled(
        &self,
        frustum_planes: &[Vec4; 6],
    ) -> Vec<ExtractedRenderable> {
        self.renderables
            .iter()
            .filter_map(|(&node, &renderable)| {
                let node_data = self.node(node).ok()?;
                let effective = self.effective_visibility(node).ok()?;
                match effective {
                    VisibilityMode::Hidden => return None,
                    VisibilityMode::AlwaysVisible => {}
                    VisibilityMode::Inherit => {
                        if node_data.world_bound.is_outside_frustum(frustum_planes) {
                            return None;
                        }
                    }
                }
                Some(ExtractedRenderable {
                    node,
                    mesh: renderable.mesh,
                    material: renderable.material,
                    world_transform: node_data.world_transform,
                    world_bound: node_data.world_bound,
                })
            })
            .collect()
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

    pub(crate) fn compute_world_bounds(
        &mut self,
        node: NodeId,
        assets: &AssetStore,
    ) -> Result<BoundingSphere> {
        let child_ids = self.children(node)?;
        let mut bound = if let Some(renderable) = self.renderables.get(&node).copied() {
            match renderable.mesh {
                rig_assets::MeshSource::Static(handle) => {
                    let mesh = assets
                        .mesh(handle)
                        .map_err(|_| SceneError::MissingMeshAsset)?;
                    mesh.local_bounds
                        .transform_by(self.node(node)?.world_transform)
                }
                rig_assets::MeshSource::Dynamic(_id) => {
                    // Use the dynamic bounds registered for this node, if any.
                    self.dynamic_bounds
                        .get(&node)
                        .copied()
                        .unwrap_or(BoundingSphere::ZERO)
                        .transform_by(self.node(node)?.world_transform)
                }
            }
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
}
