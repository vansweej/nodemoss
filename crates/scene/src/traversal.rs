//! World transform propagation.

use rig_math::Mat4;

use crate::SceneGraph;
use crate::node::{NodeId, Result};

impl SceneGraph {
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

    pub(crate) fn update_world_transforms_with_parent(
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
}
