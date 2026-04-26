//! SceneGraph struct, arena allocation, and tree topology operations.

use std::collections::HashMap;

use rig_math::BoundingSphere;

use crate::components::{CameraComponent, LightComponent, Renderable};
use crate::node::{NodeId, NodeSlot, Result, SceneError, SceneNode};

#[derive(Default)]
pub struct SceneGraph {
    pub(crate) nodes: Vec<NodeSlot>,
    pub(crate) free_list: Vec<u32>,
    pub(crate) renderables: HashMap<NodeId, Renderable>,
    pub(crate) cameras: HashMap<NodeId, CameraComponent>,
    pub(crate) lights: HashMap<NodeId, LightComponent>,
    /// Per-node local bounding spheres for dynamic meshes, updated each frame
    /// by `set_dynamic_bounds()` before `update_world_bounds()`.
    pub(crate) dynamic_bounds: HashMap<NodeId, BoundingSphere>,
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
        self.dynamic_bounds.remove(&id);

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

        self.node(parent)?;
        self.node(child)?;

        if self.is_ancestor(child, parent)? {
            return Err(SceneError::CycleDetected);
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

    fn is_ancestor(&self, ancestor: NodeId, descendant: NodeId) -> Result<bool> {
        let mut current = self.node(descendant)?.parent;
        while let Some(id) = current {
            if id == ancestor {
                return Ok(true);
            }
            current = self.node(id)?.parent;
        }
        Ok(false)
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

    pub fn reparent(&mut self, new_parent: NodeId, child: NodeId) -> Result<()> {
        self.attach_child(new_parent, child)
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

    pub fn parent(&self, node: NodeId) -> Result<Option<NodeId>> {
        Ok(self.node(node)?.parent)
    }

    pub fn ancestors(&self, node: NodeId) -> Result<Vec<NodeId>> {
        let mut out = Vec::new();
        let mut current = self.node(node)?.parent;
        while let Some(id) = current {
            out.push(id);
            current = self.node(id)?.parent;
        }
        Ok(out)
    }

    pub fn descendants(&self, node: NodeId) -> Result<Vec<NodeId>> {
        let mut out = Vec::new();
        self.collect_descendants(node, &mut out)?;
        Ok(out)
    }

    fn collect_descendants(&self, node: NodeId, out: &mut Vec<NodeId>) -> Result<()> {
        for child in self.children(node)? {
            out.push(child);
            self.collect_descendants(child, out)?;
        }
        Ok(())
    }

    pub(crate) fn slot(&self, id: NodeId) -> Result<&NodeSlot> {
        let slot = self
            .nodes
            .get(id.index as usize)
            .ok_or(SceneError::InvalidNode)?;
        if slot.generation != id.generation || slot.node.is_none() {
            return Err(SceneError::InvalidNode);
        }
        Ok(slot)
    }

    pub(crate) fn slot_mut(&mut self, id: NodeId) -> Result<&mut NodeSlot> {
        let slot = self
            .nodes
            .get_mut(id.index as usize)
            .ok_or(SceneError::InvalidNode)?;
        if slot.generation != id.generation || slot.node.is_none() {
            return Err(SceneError::InvalidNode);
        }
        Ok(slot)
    }

    pub(crate) fn node(&self, id: NodeId) -> Result<&SceneNode> {
        self.slot(id)?.node.as_ref().ok_or(SceneError::InvalidNode)
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> Result<&mut SceneNode> {
        self.slot_mut(id)?
            .node
            .as_mut()
            .ok_or(SceneError::InvalidNode)
    }

    pub(crate) fn root_nodes(&self) -> Vec<NodeId> {
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

    /// Register or update the local-space bounding sphere for a dynamic mesh node.
    ///
    /// Call this in `update()` after running Marching Cubes, before
    /// `update_all_world_bounds()`, so frustum culling uses up-to-date bounds.
    pub fn set_dynamic_bounds(&mut self, node: NodeId, bounds: BoundingSphere) -> Result<()> {
        self.node(node)?;
        self.dynamic_bounds.insert(node, bounds);
        Ok(())
    }
}
